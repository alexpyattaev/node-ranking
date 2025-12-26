#![allow(clippy::arithmetic_side_effects)]
use clap::Parser;
use log::error;
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;
use std::{collections::HashMap, net::SocketAddr, ops::AddAssign, time::Duration};
use tokio::sync::watch::Sender;

use crate::ranking_table::StakeData;

#[derive(Parser, Debug)]
#[command(name = "get_voting_node_ips")]
#[command(about = "HTTP server providing voting node IP addresses", long_about = None)]
struct Args {
    /// Listen address
    #[arg(long, env = "LISTEN_ADDR", default_value = "0.0.0.0:8080")]
    listen_addr: SocketAddr,

    /// Solana RPC URL
    #[arg(
        long,
        env = "RPC_URL",
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    rpc_url: String,

    /// Refresh interval in seconds
    #[arg(long, env = "REFRESH_INTERVAL", default_value = "300")]
    refresh_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcResponse<T> {
    result: T,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct VoteAccountsResult {
    current: Vec<VoteAccount>,
    delinquent: Vec<VoteAccount>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoteAccount {
    vote_pubkey: String,
    node_pubkey: String,
    activated_stake: u64,
}

async fn fetch_validator_ips(rpc_url: &str, api_key: Option<&str>) -> anyhow::Result<StakeData> {
    let client = reqwest::Client::new();

    // Get vote accounts
    let vote_req = RpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method: "getVoteAccounts",
        params: None,
    };

    let mut req_builder = client.post(rpc_url).json(&vote_req);
    if let Some(key) = api_key {
        req_builder = req_builder.header("X-Api-Key", key);
    }

    let response = req_builder.send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("getVoteAccounts failed with status {status}: {body}");
    }

    let vote_resp: RpcResponse<VoteAccountsResult> = response.json().await?;

    // Map node_pubkey -> list of vote_pubkeys
    let mut stake_amounts: HashMap<Pubkey, u64> = HashMap::new();
    for account in vote_resp.result.current {
        stake_amounts
            .entry(Pubkey::from_str_const(&account.node_pubkey))
            .or_default()
            .add_assign(account.activated_stake);
    }

    Ok(StakeData {
        stake_amounts,
        ..Default::default()
    })
}

pub async fn stakes_refresh_loop(
    rpc_url: String,
    api_key: Option<String>,
    sender: Sender<StakeData>,
    interval: Duration,
) -> anyhow::Result<()> {
    loop {
        match fetch_validator_ips(&rpc_url, api_key.as_deref()).await {
            Ok(ip_data) => {
                sender.send(ip_data)?;
            }
            Err(e) => {
                error!("Failed to fetch validator IPs: {}", e);
            }
        }
        tokio::time::sleep(interval).await;
    }
}
