use std::{collections::HashMap, time::Instant};

use solana_pubkey::Pubkey;

use crate::gossip_spy::Ports;

#[derive(Debug)]
pub struct GossipData {
    pub gossip_data: HashMap<Pubkey, Ports>,
    pub last_update: Instant,
}

impl Default for GossipData {
    fn default() -> Self {
        Self {
            gossip_data: HashMap::default(),
            last_update: Instant::now(),
        }
    }
}

#[derive(Debug)]
pub struct StakeData {
    pub stake_amounts: HashMap<Pubkey, u64>,
    pub last_update: Instant,
}

impl Default for StakeData {
    fn default() -> Self {
        Self {
            stake_amounts: HashMap::default(),
            last_update: Instant::now(),
        }
    }
}
