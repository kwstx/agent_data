use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::time::{self, Duration, Instant};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use rand::Rng;
use crate::{IngestionNode, DataSource, StateValue};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub average_latency_ms: f64,
    pub max_latency_ms: u64,
    pub current_throughput: f64, // req/sec
    pub quorum_consistency: f64, // 0.0 to 1.0
}

/// Adversarial Data Source that can simulate faults
pub struct AdversarialSource {
    pub base_name: String,
    pub chaos: Arc<ChaosGovernor>,
}

pub struct ChaosGovernor {
    pub latency_range: tokio::sync::RwLock<(u64, u64)>,
    pub corruption_rate: tokio::sync::RwLock<f64>,
}

impl ChaosGovernor {
    pub fn new() -> Self {
        Self {
            latency_range: tokio::sync::RwLock::new((0, 0)),
            corruption_rate: tokio::sync::RwLock::new(0.0),
        }
    }
}

#[async_trait]
impl DataSource for AdversarialSource {
    fn name(&self) -> String { self.base_name.clone() }
    
    async fn fetch_update(&self) -> Result<Vec<(String, f64)>> {
        // 1. Inject Latency
        let (min, max) = *self.chaos.latency_range.read().await;
        if max > 0 {
            let delay = if min == max { min } else { rand::thread_rng().gen_range(min..=max) };
            time::sleep(Duration::from_millis(delay)).await;
        }

        // 2. Inject Corruption
        let rate = *self.chaos.corruption_rate.read().await;
        let mut val = 2500.0 + (rand::random::<f64>() * 50.0);
        
        if rand::thread_rng().gen_bool(rate) {
            val *= 10.0; // Corrupt data by an order of magnitude
        }

        Ok(vec![("ETH/USD".to_string(), val)])
    }
}

pub struct StressTester {
    metrics: Arc<DashMap<String, Vec<u128>>>, // node_id -> latencies
    chaos: Arc<ChaosGovernor>,
}

impl StressTester {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(DashMap::new()),
            chaos: Arc::new(ChaosGovernor::new()),
        }
    }

    pub async fn apply_chaos(&self, latency: (u64, u64), corruption: f64) {
        let mut lat = self.chaos.latency_range.write().await;
        *lat = latency;
        let mut corr = self.chaos.corruption_rate.write().await;
        *corr = corruption;
    }

    pub async fn run_load_test(&self, target_url: &str, concurrent_agents: usize, duration_secs: u64) {
        log::info!("Starting Stress Test: {} agents for {} seconds", concurrent_agents, duration_secs);
        
        let client = reqwest::Client::new();
        let start_time = Instant::now();
        let end_time = start_time + Duration::from_secs(duration_secs);
        
        let counter = Arc::new(AtomicU64::new(0));
        let latencies = Arc::new(tokio::sync::RwLock::new(Vec::new()));

        let mut handles = Vec::new();

        for i in 0..concurrent_agents {
            let client = client.clone();
            let target = target_url.to_string();
            let counter = counter.clone();
            let latencies = latencies.clone();
            let end_time = end_time.clone();

            handles.push(tokio::spawn(async move {
                while Instant::now() < end_time {
                    let req_start = Instant::now();
                    let res = client.get(&format!("{}/state/ETH/USD", target)).send().await;
                    
                    if res.is_ok() {
                        let duration = req_start.elapsed().as_millis();
                        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        latencies.write().await.push(duration);
                    }
                    
                    // Small sleep to prevent immediate exhaustion of local ports if needed, 
                    // but we want high frequency.
                    tokio::task::yield_now().await;
                }
            }));
        }

        for h in handles {
            let _ = h.await;
        }

        let total_reqs = counter.load(std::sync::atomic::Ordering::SeqCst);
        let elapsed = start_time.elapsed().as_secs_f64();
        let lats = latencies.read().await;
        let avg_lat = if !lats.is_empty() {
            lats.iter().sum::<u128>() as f64 / lats.len() as f64
        } else {
            0.0
        };

        log::info!("--- Stress Test Results ---");
        log::info!("Total Requests: {}", total_reqs);
        log::info!("Throughput: {:.2} req/sec", total_reqs as f64 / elapsed);
        log::info!("Average Latency: {:.2} ms", avg_lat);
    }
}
