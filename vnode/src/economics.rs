use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStats {
    pub node_id: String,
    pub stake: f64,
    pub reputation: f64, // 0.0 to 1.0
    pub uptime_secs: u64,
    pub successful_contributions: u64,
    pub slashes: u32,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    pub node_id: String,
    pub epoch: u64,
    pub reason: String,
    pub amount_slashed: f64,
}

pub struct StakingManager {
    pub nodes: Arc<RwLock<HashMap<String, NodeStats>>>,
    pub min_stake: f64,
    pub slashing_threshold: f64, // Deviation percentage
}

impl StakingManager {
    pub fn new(min_stake: f64) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            min_stake,
            slashing_threshold: 0.1, // 10% deviation triggers investigation/slashing
        }
    }

    pub async fn register_node(&self, node_id: &str, stake: f64) -> Result<(), String> {
        if stake < self.min_stake {
            return Err(format!("Stake {} is below minimum {}", stake, self.min_stake));
        }

        let mut nodes = self.nodes.write().await;
        nodes.insert(node_id.to_string(), NodeStats {
            node_id: node_id.to_string(),
            stake,
            reputation: 1.0,
            uptime_secs: 0,
            successful_contributions: 0,
            slashes: 0,
            last_seen: Utc::now(),
        });
        Ok(())
    }

    pub async fn update_performance(&self, node_id: &str, is_consistent: bool, _latency_ms: u64) {
        let mut nodes = self.nodes.write().await;
        if let Some(stats) = nodes.get_mut(node_id) {
            stats.last_seen = Utc::now();
            if is_consistent {
                stats.successful_contributions += 1;
                // Gradually recover reputation if it was lost
                stats.reputation = (stats.reputation + 0.01).min(1.0);
            } else {
                stats.reputation = (stats.reputation - 0.1).max(0.0);
            }
            // Logic for uptime would need a periodic heartbeat or tracking
        }
    }

    pub async fn slash_node(&self, node_id: &str, epoch: u64, reason: &str, penalty_pct: f64) -> Option<SlashingEvent> {
        let mut nodes = self.nodes.write().await;
        if let Some(stats) = nodes.get_mut(node_id) {
            let penalty = stats.stake * penalty_pct;
            stats.stake -= penalty;
            stats.slashes += 1;
            stats.reputation = (stats.reputation - 0.3).max(0.0);
            
            Some(SlashingEvent {
                node_id: node_id.to_string(),
                epoch,
                reason: reason.to_string(),
                amount_slashed: penalty,
            })
        } else {
            None
        }
    }

    pub async fn calculate_rewards(&self, reward_pool: f64) -> HashMap<String, f64> {
        let nodes = self.nodes.read().await;
        let total_weight: f64 = nodes.values()
            .map(|n| n.stake * n.reputation)
            .sum();

        let mut rewards = HashMap::new();
        if total_weight > 0.0 {
            for (id, stats) in nodes.iter() {
                let node_weight = stats.stake * stats.reputation;
                let reward = (node_weight / total_weight) * reward_pool;
                rewards.insert(id.clone(), reward);
            }
        }
        rewards
    }

    pub async fn distribute_rewards(&self, rewards: HashMap<String, f64>) {
        let mut nodes = self.nodes.write().await;
        for (id, amount) in rewards {
            if let Some(stats) = nodes.get_mut(&id) {
                stats.stake += amount;
            }
        }
    }
}
