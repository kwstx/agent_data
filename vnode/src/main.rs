use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{self, Duration};
use chrono::{Utc, DateTime};
use anyhow::{Result, anyhow};
use ed25519_dalek::{SigningKey, Signer, VerifyingKey, Signature};
use rand::rngs::OsRng;
use async_trait::async_trait;
use sha2::{Sha256, Digest};
use rs_merkle::{MerkleTree, Hasher};
use std::collections::BTreeMap;

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
}

pub type GlobalState = Arc<DashMap<String, StateValue>>;

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
}

impl IngestionNode {
    fn new(id: &str, interval_ms: u64) -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        
        Self {
            id: id.to_string(),
            state: Arc::new(DashMap::new()),
            signing_key,
            interval_ms,
            peer_observations: Arc::new(DashMap::new()),
        }
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
    fn build_merkle_root(values: &BTreeMap<String, f64>) -> String {
        let leaves: Vec<[u8; 32]> = values.iter()
            .map(|(k, v)| {
                let leaf_data = format!("{}:{}", k, v);
                Sha256Hasher::hash(leaf_data.as_bytes())
            })
            .collect();
        
        let tree = MerkleTree::<Sha256Hasher>::from_leaves(&leaves);
        hex::encode(tree.root().unwrap_or([0u8; 32]))
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
            for (key, mut values) in value_map {
                let median = Self::calculate_median(&mut values);
                aggregated_values.insert(key, median);
            }

            // 4. Construct Merkle Tree over aggregated state
            let root = Self::build_merkle_root(&aggregated_values);

            // 5. Produce Quorum Snapshot (Evidence of majority)
            // In a real system, we'd verify multiple node signatures for the same root
            let snapshot = QuorumSnapshot {
                epoch,
                merkle_root: root.clone(),
                values: aggregated_values.clone(),
                signatures: all_observations.iter().map(|o| o.signature.clone()).collect(),
            };

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
        
        for pid in peer_ids {
            let offset = (rand::random::<f64>() - 0.5) * 10.0;
            // Simplified peer observation (not actually signing here for brevity in demo)
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
    node.run_consensus_loop().await?;

    Ok(())
}
