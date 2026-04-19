use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{self, Duration};
use chrono::{Utc, DateTime};
use anyhow::{Result, anyhow};
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

mod economics;
use economics::{StakingManager, NodeStats, SlashingEvent};

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
    pub value: f64,
    pub timestamp: DateTime<Utc>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedObservation {
    pub node_id: String,
    pub key: String,
    pub value: f64,
    pub timestamp: DateTime<Utc>,
    pub signature: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumSnapshot {
    pub epoch: u64,
    pub merkle_root: String,
    pub values: BTreeMap<String, f64>, // Aggregated values (e.g. medians)
    pub signatures: Vec<String>,      // Evidence of node agreement
    pub slashing_events: Vec<SlashingEvent>,
}

pub type GlobalState = Arc<DashMap<String, StateValue>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub key: String,
    pub value: f64,
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
    async fn fetch_update(&self) -> Result<Vec<(String, f64)>>;
}

// --- Mock Sources ---

pub struct ExchangeApi {
    pub name: String,
}

#[async_trait]
impl DataSource for ExchangeApi {
    fn name(&self) -> String { self.name.clone() }
    async fn fetch_update(&self) -> Result<Vec<(String, f64)>> {
        let val = 2500.0 + (rand::random::<f64>() * 50.0);
        Ok(vec![("ETH/USD".to_string(), val)])
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
    fn sign_observation(&self, key: &str, value: f64) -> SignedObservation {
        let timestamp = Utc::now();
        let message = format!("{}:{}:{}:{}", self.id, key, value, timestamp.to_rfc3339());
        let signature = self.signing_key.sign(message.as_bytes());
        
        SignedObservation {
            node_id: self.id.clone(),
            key: key.to_string(),
            value,
            timestamp,
            signature: hex::encode(signature.to_bytes()),
            public_key: hex::encode(VerifyingKey::from(&self.signing_key).to_bytes()),
        }
    }

    /// Deterministic aggregation: calculates median of a list of values
    fn calculate_median(values: &mut Vec<f64>) -> f64 {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mid = values.len() / 2;
        if values.len() % 2 == 0 {
            (values[mid - 1] + values[mid]) / 2.0
        } else {
            values[mid]
        }
    }

    /// Performs Merkle tree construction over aggregated state
    fn build_merkle_tree(values: &BTreeMap<String, f64>) -> (MerkleTree<Sha256Hasher>, BTreeMap<String, usize>) {
        let mut key_to_index = BTreeMap::new();
        let leaves: Vec<[u8; 32]> = values.iter().enumerate()
            .map(|(i, (k, v))| {
                key_to_index.insert(k.clone(), i);
                let leaf_data = format!("{}:{}", k, v);
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
                let obs = self.sign_observation(entry.key(), entry.value().value);
                local_observations.push(obs);
            }

            // 2. Simulate receiving peer observations (Mocking 2 other nodes)
            let mut all_observations = local_observations.clone();
            all_observations.extend(self.simulate_peer_data());

            // 3. Deterministic Aggregation (Median)
            let mut value_map: BTreeMap<String, Vec<f64>> = BTreeMap::new();
            for obs in &all_observations {
                value_map.entry(obs.key.clone()).or_default().push(obs.value);
            }

            let mut aggregated_values = BTreeMap::new();
            for (key, values) in &value_map {
                let mut v = values.clone();
                let median = Self::calculate_median(&mut v);
                aggregated_values.insert(key.clone(), median);
            }

            // 4. Construct Merkle Tree over aggregated state
            let (tree, key_to_index) = Self::build_merkle_tree(&aggregated_values);
            let root = hex::encode(tree.root().unwrap_or([0u8; 32]));

            // 5. Evaluate Node Performance and Check for Slashing
            let mut slashing_events = Vec::new();
            for (node_id, _values) in &value_map {
                // If we have a consensus value (median) for this key
                if let Some(&median) = aggregated_values.get(node_id) {
                    for obs in &all_observations {
                        if &obs.key == node_id {
                            let deviation = (obs.value - median).abs() / median;
                            let is_consistent = deviation < self.staking_manager.slashing_threshold;
                            
                            self.staking_manager.update_performance(&obs.node_id, is_consistent, 0).await;
                            
                            if !is_consistent {
                                if let Some(event) = self.staking_manager.slash_node(
                                    &obs.node_id, 
                                    epoch, 
                                    &format!("Value deviation: {:.2}%", deviation * 100.0),
                                    0.05 // Slash 5% for malicious/incorrect data
                                ).await {
                                    slashing_events.push(event);
                                    log::warn!("Node {} SLASHED for deviation: {:.2}%", obs.node_id, deviation * 100.0);
                                }
                            }
                        }
                    }
                }
            }

            // 6. Distribute Rewards
            let rewards = self.staking_manager.calculate_rewards(1.0).await; // 1.0 units per epoch
            self.staking_manager.distribute_rewards(rewards).await;

            // 7. Produce Quorum Snapshot
            let message = format!("epoch:{}:root:{}", epoch, root);
            let signature = self.signing_key.sign(message.as_bytes());
            let node_signature = hex::encode(signature.to_bytes());

            let mut all_signatures = vec![node_signature];
            all_signatures.push("peer_alpha_sig_placeholder".to_string());
            all_signatures.push("peer_beta_sig_placeholder".to_string());

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

    fn simulate_peer_data(&self) -> Vec<SignedObservation> {
        // Mocking peer nodes providing slightly different data
        let mut peers = Vec::new();
        let peer_ids = vec!["node_beta", "node_gamma"];
        
        for (_i, pid) in peer_ids.iter().enumerate() {
            let mut offset = (rand::random::<f64>() - 0.5) * 10.0;
            
            // Periodically inject "malicious" or "stale" data from node_gamma to demonstrate slashing
            if *pid == "node_gamma" && rand::random::<f64>() < 0.2 {
                offset = 100.0; // Significant deviation
            }

            peers.push(SignedObservation {
                node_id: pid.to_string(),
                key: "ETH/USD".to_string(),
                value: 2500.0 + offset, 
                timestamp: Utc::now(),
                signature: "peer_sig_placeholder".to_string(),
                public_key: "peer_pk_placeholder".to_string(),
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
        .route("/economics", get(get_economics))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(node);

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
