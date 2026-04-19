use std::sync::Arc;
use tokio::time::{self, Duration};
use chrono::{Utc, DateTime};
use anyhow::Result;
use ed25519_dalek::{SigningKey, Signer, VerifyingKey};
use rand::rngs::OsRng;
use async_trait::async_trait;
use sha2::{Sha256, Digest};
use rs_merkle::{MerkleTree, Hasher};
use std::collections::BTreeMap;
use axum::{
    extract::{ws::{WebSocket, Message, WebSocketUpgrade}, Path, State},
    response::IntoResponse,
    routing::get,
    Router,
    Json,
};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use serde::{Serialize, Deserialize};
use serde_json;
use dashmap::DashMap;

mod economics;
mod simulation;

use economics::{StakingManager, NodeStats, SlashingEvent};
use simulation::{AdversarialSource, ChaosGovernor, StressTester};

// --- Cryptographic & Merkle Setup ---

#[derive(Clone)]
pub struct Sha256Hasher {}

impl Hasher for Sha256Hasher {
    type Hash = [u8; 32];
    fn hash(data: &[u8]) -> Self::Hash {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }
}

// --- State Definitions ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateValue {
    pub value: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedObservation {
    pub node_id: String,
    pub key: String,
    pub value: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub signature: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumSnapshot {
    pub epoch: u64,
    pub merkle_root: String,
    pub values: BTreeMap<String, serde_json::Value>, // Aggregated values (e.g. medians or modes)
    pub signatures: Vec<String>,                    // Evidence of node agreement
    pub slashing_events: Vec<SlashingEvent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DeploymentPhase {
    SingleChain,        // Phase 1: Semi-trusted cluster, basic aggregation
    MultiNodeQuorum,    // Phase 2: Quorum enforcement, on-chain verification focus
    FullyDecentralized, // Phase 3: Staking, slashing, permissionless
}

pub type GlobalState = Arc<DashMap<String, StateValue>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub key: String,
    pub value: serde_json::Value,
    pub proof: ProofData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofData {
    pub merkle_root: String,
    pub proof_path: Vec<(String, bool)>, // (hash, is_left)
    pub signatures: Vec<String>,
}

pub struct InternalSnapshot {
    pub snapshot: QuorumSnapshot,
    pub tree: MerkleTree<Sha256Hasher>,
    pub key_to_index: BTreeMap<String, usize>,
}

// --- Source Abstraction ---

#[async_trait]
pub trait DataSource: Send + Sync {
    fn name(&self) -> String;
    async fn fetch_update(&self) -> Result<Vec<(String, serde_json::Value)>>;
}

// --- Mock Sources ---

pub struct ExchangeApi {
    pub name: String,
}

#[async_trait]
impl DataSource for ExchangeApi {
    fn name(&self) -> String { self.name.clone() }
    async fn fetch_update(&self) -> Result<Vec<(String, serde_json::Value)>> {
        let val = 2500.0 + (rand::random::<f64>() * 50.0);
        Ok(vec![("ETH/USD".to_string(), serde_json::json!(val))])
    }
}

// --- Node Implementation ---

struct IngestionNode {
    id: String,
    state: GlobalState,
    signing_key: SigningKey,
    interval_ms: u64,
    // Simulator for peer observations
    peer_observations: Arc<DashMap<u64, Vec<SignedObservation>>>,
    // Latest verified state
    latest_snapshot: Arc<tokio::sync::RwLock<Option<InternalSnapshot>>>,
    tx: broadcast::Sender<QuorumSnapshot>,
    pub staking_manager: Arc<StakingManager>,
    pub chaos: Arc<ChaosGovernor>,
    pub phase: Arc<tokio::sync::RwLock<DeploymentPhase>>,
}

impl IngestionNode {
    fn new(id: &str, interval_ms: u64) -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        
        let (tx, _) = broadcast::channel(100);
        
        Self {
            id: id.to_string(),
            state: Arc::new(DashMap::new()),
            signing_key,
            interval_ms,
            peer_observations: Arc::new(DashMap::new()),
            latest_snapshot: Arc::new(tokio::sync::RwLock::new(None)),
            tx,
            staking_manager: Arc::new(StakingManager::new(100.0)),
            chaos: Arc::new(ChaosGovernor::new()),
            phase: Arc::new(tokio::sync::RwLock::new(DeploymentPhase::SingleChain)), // Start at Phase 1
        }
    }

    fn public_key(&self) -> String {
        hex::encode(VerifyingKey::from(&self.signing_key).to_bytes())
    }

    async fn run_source(&self, source: Arc<dyn DataSource>) -> Result<()> {
        let state = self.state.clone();
        let interval = self.interval_ms;
        
        tokio::spawn(async move {
            let mut ticker = time::interval(Duration::from_millis(interval));
            loop {
                ticker.tick().await;
                if let Ok(updates) = source.fetch_update().await {
                    for (key, value) in updates {
                        state.insert(key, StateValue {
                            value,
                            timestamp: Utc::now(),
                            source: source.name(),
                        });
                    }
                }
            }
        });
        Ok(())
    }

    /// Signs an observation for a given key and value
    fn sign_observation(&self, key: &str, value: &serde_json::Value) -> SignedObservation {
        let timestamp = Utc::now();
        let val_str = serde_json::to_string(value).unwrap_or_default();
        let message = format!("{}:{}:{}:{}", self.id, key, val_str, timestamp.to_rfc3339());
        let signature = self.signing_key.sign(message.as_bytes());
        
        SignedObservation {
            node_id: self.id.clone(),
            key: key.to_string(),
            value: value.clone(),
            timestamp,
            signature: hex::encode(signature.to_bytes()),
            public_key: hex::encode(VerifyingKey::from(&self.signing_key).to_bytes()),
        }
    }

    /// Generalized aggregation: calculates median for numbers, mode for others
    fn aggregate_values(values: &[serde_json::Value]) -> serde_json::Value {
        if values.is_empty() { return serde_json::Value::Null; }

        // Check if all are numbers
        let mut numbers = Vec::new();
        for v in values {
            if let Some(n) = v.as_f64() {
                numbers.push(n);
            }
        }

        if numbers.len() == values.len() {
            // All numbers: calculate median
            numbers.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mid = numbers.len() / 2;
            let result = if numbers.len() % 2 == 0 {
                (numbers[mid - 1] + numbers[mid]) / 2.0
            } else {
                numbers[mid]
            };
            serde_json::json!(result)
        } else {
            // Mixed or non-numbers: calculate mode
            let mut counts = BTreeMap::new();
            for v in values {
                let serialized = serde_json::to_string(v).unwrap();
                *counts.entry(serialized).or_insert(0) += 1;
            }
            let mode_serialized = counts.into_iter()
                .max_by_key(|&(_, count)| count)
                .map(|(val, _)| val)
                .unwrap();
            
            serde_json::from_str(&mode_serialized).unwrap()
        }
    }

    /// Performs Merkle tree construction over aggregated state
    fn build_merkle_tree(values: &BTreeMap<String, serde_json::Value>) -> (MerkleTree<Sha256Hasher>, BTreeMap<String, usize>) {
        let mut key_to_index = BTreeMap::new();
        let leaves: Vec<[u8; 32]> = values.iter().enumerate()
            .map(|(i, (k, v))| {
                key_to_index.insert(k.clone(), i);
                let val_str = serde_json::to_string(v).unwrap_or_default();
                let leaf_data = format!("{}:{}", k, val_str);
                Sha256Hasher::hash(leaf_data.as_bytes())
            })
            .collect();
        
        (MerkleTree::<Sha256Hasher>::from_leaves(&leaves), key_to_index)
    }

    async fn run_consensus_loop(&self) -> Result<()> {
        let mut ticker = time::interval(Duration::from_millis(1000));
        let mut epoch = 0;

        loop {
            ticker.tick().await;
            epoch += 1;

            if self.state.is_empty() { continue; }

            log::info!("--- Epoch {} ---", epoch);

            // 1. Collect local observations and sign them
            let mut local_observations = Vec::new();
            for entry in self.state.iter() {
                let obs = self.sign_observation(entry.key(), &entry.value().value);
                local_observations.push(obs);
            }

            // 2. Simulate receiving peer observations from registered nodes
            let mut all_observations = local_observations.clone();
            all_observations.extend(self.simulate_registered_peer_data().await);

            // 3. Generalized Aggregation
            let mut value_map: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
            for obs in &all_observations {
                value_map.entry(obs.key.clone()).or_default().push(obs.value.clone());
            }

            let mut aggregated_values = BTreeMap::new();
            for (key, values) in &value_map {
                let consensus_value = Self::aggregate_values(values);
                aggregated_values.insert(key.clone(), consensus_value);
            }

            // 4. Construct Merkle Tree over aggregated state
            let (tree, key_to_index) = Self::build_merkle_tree(&aggregated_values);
            let root = hex::encode(tree.root().unwrap_or([0u8; 32]));

            // 5. Evaluate Node Performance and Check for Slashing (Only in Phase 2/3)
            let current_phase = *self.phase.read().await;
            let mut slashing_events = Vec::new();

            if current_phase != DeploymentPhase::SingleChain {
                for (key, consensus_val) in &aggregated_values {
                    // Only perform deviation slashing if it's a number
                    if let Some(median) = consensus_val.as_f64() {
                        for obs in &all_observations {
                            if &obs.key == key {
                                if let Some(obs_val) = obs.value.as_f64() {
                                    let deviation = (obs_val - median).abs() / median;
                                    let is_consistent = deviation < self.staking_manager.slashing_threshold;
                                    
                                    self.staking_manager.update_performance(&obs.node_id, is_consistent, 0).await;
                                    
                                    if !is_consistent && current_phase == DeploymentPhase::FullyDecentralized {
                                        if let Some(event) = self.staking_manager.slash_node(
                                            &obs.node_id, 
                                            epoch, 
                                            &format!("Value deviation: {:.2}%", deviation * 100.0),
                                            0.05
                                        ).await {
                                            slashing_events.push(event);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // For non-numbers (strings/bools), check for exact match
                        for obs in &all_observations {
                            if &obs.key == key {
                                let is_consistent = &obs.value == consensus_val;
                                self.staking_manager.update_performance(&obs.node_id, is_consistent, 0).await;
                                
                                if !is_consistent && current_phase == DeploymentPhase::FullyDecentralized {
                                    if let Some(event) = self.staking_manager.slash_node(
                                        &obs.node_id, 
                                        epoch, 
                                        "Value mismatch (mode consensus)",
                                        0.05
                                    ).await {
                                        slashing_events.push(event);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 6. Distribute Rewards (Only in Phase 3)
            if current_phase == DeploymentPhase::FullyDecentralized {
                let rewards = self.staking_manager.calculate_rewards(1.0).await;
                self.staking_manager.distribute_rewards(rewards).await;
            }

            // 7. Produce Quorum Snapshot
            let message = format!("epoch:{}:root:{}", epoch, root);
            let signature = self.signing_key.sign(message.as_bytes());
            let node_signature = hex::encode(signature.to_bytes());

            let mut all_signatures = vec![node_signature];
            
            // In Phase 2+, we require collective signatures.
            if current_phase != DeploymentPhase::SingleChain {
                let nodes = self.staking_manager.nodes.read().await;
                for (id, _stats) in nodes.iter() {
                    if id != &self.id {
                        // Mocking signature from registered node
                        all_signatures.push(format!("{}_sig_placeholder", id));
                    }
                }
            }

            let snapshot = QuorumSnapshot {
                epoch,
                merkle_root: root.clone(),
                values: aggregated_values.clone(),
                signatures: all_signatures,
                slashing_events,
            };

            // Update shared state and broadcast
            {
                let mut lock = self.latest_snapshot.write().await;
                *lock = Some(InternalSnapshot {
                    snapshot: snapshot.clone(),
                    tree,
                    key_to_index,
                });
            }
            let _ = self.tx.send(snapshot.clone());

            log::info!("Quorum Achieved for Epoch {}: Root={}", epoch, root);
            for (k, v) in &snapshot.values {
                log::info!("  -> {}: {:.4} (based on {} observations)", k, v, all_observations.len());
            }
        }
    }

    async fn simulate_registered_peer_data(&self) -> Vec<SignedObservation> {
        let mut peers = Vec::new();
        let nodes = self.staking_manager.nodes.read().await;
        
        for (id, _stats) in nodes.iter() {
            if id == &self.id { continue; }
            
            let mut offset = (rand::random::<f64>() - 0.5) * 10.0;
            
            // Periodically inject "malicious" or "stale" data from node_gamma to demonstrate slashing
            if id == "node_gamma" && rand::random::<f64>() < 0.2 {
                offset = 100.0; // Significant deviation
            }

            peers.push(SignedObservation {
                node_id: id.clone(),
                key: "ETH/USD".to_string(),
                value: serde_json::json!(2500.0 + offset), 
                timestamp: Utc::now(),
                signature: format!("{}_sig", id),
                public_key: format!("{}_pk", id),
            });
        }
        peers
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    
    log::info!("Starting Verifiable Data Node with Quorum Aggregation...");
    
    let node = Arc::new(IngestionNode::new("node_alpha", 250));
    
    let source1 = Arc::new(ExchangeApi { name: "Feed_A".to_string() });
    node.run_source(source1).await?;

    // Consensus loop handles signing, median aggregation, and Merkle root calculation
    let node_loop = node.clone();

    // Register initial nodes in the staking system
    node.staking_manager.register_node("node_alpha", 1000.0).await.unwrap();
    node.staking_manager.register_node("node_beta", 1000.0).await.unwrap();
    node.staking_manager.register_node("node_gamma", 1000.0).await.unwrap();

    tokio::spawn(async move {
        if let Err(e) = node_loop.run_consensus_loop().await {
            log::error!("Consensus loop error: {}", e);
        }
    });

    // --- API Server ---

    let app = Router::new()
        .route("/state", get(get_all_state))
        .route("/state/:key", get(get_key_state))
        .route("/nodes", get(get_nodes))
        .route("/nodes/join", axum::routing::post(onboard_node))
        .route("/economics", get(get_economics))
        .route("/verify/proof", axum::routing::post(verify_proof))
        .route("/simulation/chaos", axum::routing::post(set_chaos))
        .route("/simulation/load", axum::routing::post(run_load))
        .route("/simulation/phase", axum::routing::post(set_phase))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(node.clone());

    // Initialize StressTester
    let _stress_tester = Arc::new(StressTester::new());
    
    // Setup Ingestion with Adversarial Source
    let adv_source = Arc::new(AdversarialSource {
        base_name: "Feed_Adversarial".to_string(),
        chaos: node.chaos.clone(),
    });
    node.run_source(adv_source).await?;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    log::info!("API Server listening on http://localhost:3000");
    axum::serve(listener, app).await?;

    Ok(())
}

// --- API Handlers ---

async fn get_all_state(State(node): State<Arc<IngestionNode>>) -> impl IntoResponse {
    let lock = node.latest_snapshot.read().await;
    if let Some(internal) = &*lock {
        Json(internal.snapshot.clone()).into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "No snapshot available").into_response()
    }
}

async fn get_key_state(
    State(node): State<Arc<IngestionNode>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let lock = node.latest_snapshot.read().await;
    if let Some(internal) = &*lock {
        if let Some(&index) = internal.key_to_index.get(&key) {
            let value = *internal.snapshot.values.get(&key).unwrap();
            let proof = internal.tree.proof(&[index]);
            let hashes = proof.proof_hashes();
            
            let mut proof_path = Vec::new();
            let mut curr_index = index;
            for h in hashes {
                // If index is even, sibling is at index + 1 (right)
                // If index is odd, sibling is at index - 1 (left)
                let is_left = curr_index % 2 == 1;
                proof_path.push((hex::encode(h), is_left));
                curr_index /= 2;
            }
            
            Json(QueryResponse {
                key,
                value,
                proof: ProofData {
                    merkle_root: internal.snapshot.merkle_root.clone(),
                    proof_path,
                    signatures: internal.snapshot.signatures.clone(),
                },
            }).into_response()
        } else {
            (axum::http::StatusCode::NOT_FOUND, "Key not found in state").into_response()
        }
    } else {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, "No snapshot available").into_response()
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(node): State<Arc<IngestionNode>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, node))
}

async fn handle_socket(mut socket: WebSocket, node: Arc<IngestionNode>) {
    let mut rx = node.tx.subscribe();
    
    // Send latest state immediately if available
    {
        let lock = node.latest_snapshot.read().await;
        if let Some(internal) = &*lock {
            let msg = serde_json::to_string(&internal.snapshot).unwrap();
            if socket.send(Message::Text(msg)).await.is_err() {
                return;
            }
        }
    }

    while let Ok(snapshot) = rx.recv().await {
        let msg = serde_json::to_string(&snapshot).unwrap();
        if socket.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}

async fn get_nodes(State(node): State<Arc<IngestionNode>>) -> impl IntoResponse {
    Json(vec![node.public_key()]).into_response()
}

async fn get_economics(State(node): State<Arc<IngestionNode>>) -> impl IntoResponse {
    let nodes = node.staking_manager.nodes.read().await;
    let stats: Vec<NodeStats> = nodes.values().cloned().collect();
    Json(stats).into_response()
}

#[derive(Deserialize)]
struct OnboardRequest {
    node_id: String,
    stake: f64,
}

async fn onboard_node(
    State(node): State<Arc<IngestionNode>>,
    Json(payload): Json<OnboardRequest>,
) -> impl IntoResponse {
    match node.staking_manager.register_node(&payload.node_id, payload.stake).await {
        Ok(_) => {
            log::info!("Permissionless Onboarding: Node {} joined with stake {}", payload.node_id, payload.stake);
            (axum::http::StatusCode::OK, "Node joined").into_response()
        },
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
struct VerifyRequest {
    key: String,
    value: serde_json::Value,
    root: String,
    proof_path: Vec<(String, bool)>,
}

async fn verify_proof(
    Json(payload): Json<VerifyRequest>,
) -> impl IntoResponse {
    // Reconstruct the leaf
    let val_str = serde_json::to_string(&payload.value).unwrap_or_default();
    let leaf_data = format!("{}:{}", payload.key, val_str);
    let mut current_hash = Sha256Hasher::hash(leaf_data.as_bytes());

    for (hash_hex, is_left) in payload.proof_path {
        let sibling = hex::decode(hash_hex).unwrap();
        let mut hasher = Sha256::new();
        if is_left {
            hasher.update(&sibling);
            hasher.update(&current_hash);
        } else {
            hasher.update(&current_hash);
            hasher.update(&sibling);
        }
        current_hash = hasher.finalize().into();
    }

    let calculated_root = hex::encode(current_hash);
    let is_valid = calculated_root == payload.root;

    Json(serde_json::json!({
        "valid": is_valid,
        "calculated_root": calculated_root,
        "provided_root": payload.root
    })).into_response()
}

#[derive(Deserialize)]
struct ChaosRequest {
    latency_min: u64,
    latency_max: u64,
    corruption_rate: f64,
}

async fn set_chaos(
    State(node): State<Arc<IngestionNode>>,
    Json(payload): Json<ChaosRequest>,
) -> impl IntoResponse {
    let mut lat = node.chaos.latency_range.write().await;
    *lat = (payload.latency_min, payload.latency_max);
    let mut corr = node.chaos.corruption_rate.write().await;
    *corr = payload.corruption_rate;
    
    log::info!("Chaos Parameters Updated: Latency {}ms-{}ms, Corruption {}%", 
        payload.latency_min, payload.latency_max, payload.corruption_rate * 100.0);
    
    axum::http::StatusCode::OK
}

#[derive(Deserialize)]
struct LoadRequest {
    agents: usize,
    duration_secs: u64,
}

async fn run_load(
    State(_node): State<Arc<IngestionNode>>,
    Json(payload): Json<LoadRequest>,
) -> impl IntoResponse {
    let stress_tester = StressTester::new();
    tokio::spawn(async move {
        stress_tester.run_load_test("http://localhost:3000", payload.agents, payload.duration_secs).await;
    });
    
    axum::http::StatusCode::ACCEPTED
}

async fn set_phase(
    State(node): State<Arc<IngestionNode>>,
    Json(phase): Json<DeploymentPhase>,
) -> impl IntoResponse {
    let mut p = node.phase.write().await;
    *p = phase;
    log::info!("Deployment Phase Updated to: {:?}", phase);
    axum::http::StatusCode::OK
}
