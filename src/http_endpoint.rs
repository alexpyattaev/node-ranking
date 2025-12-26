use crate::{
    gossip_spy::Ports,
    ranking_table::{GossipData, StakeData},
};
use anyhow::Context;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use log::info;
use serde::{Deserialize, Serialize};
use solana_native_token::LAMPORTS_PER_SOL;
use solana_pubkey::Pubkey;
use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{net::TcpListener, sync::watch::Receiver};

type AppState = (Receiver<GossipData>, Receiver<StakeData>);
pub async fn run_http(state: AppState, listen_addr: SocketAddr) -> anyhow::Result<()> {
    // Start HTTP server
    let listener = TcpListener::bind(listen_addr).await?;
    info!("Listening on {listen_addr}");

    let app = Router::new()
        .route("/", get(get_root))
        .route("/v1/allowlist", get(get_allowlist_full))
        .route("/v1/allowlist/full", get(get_allowlist_full))
        .route("/v1/allowlist/short", get(get_allowlist_short))
        .route("/v1/health", get(get_health))
        .with_state(state);

    axum::serve(listener, app)
        .await
        .context("Axum encountered an error")
}

async fn get_root() -> impl IntoResponse {
    String::from("This is a private server, get lost")
}

#[derive(Debug, Serialize, Clone)]
pub struct Descriptor {
    protocol: &'static str,
    address: SocketAddr,
    max_mbps: u64,
    staked_only: bool,
}
impl Descriptor {
    fn quic(address: SocketAddr) -> Self {
        Self {
            protocol: "QUIC",
            address,
            max_mbps: 100,
            staked_only: false,
        }
    }

    fn udp_gossip(address: SocketAddr) -> Self {
        Self {
            protocol: "UDP",
            address,
            max_mbps: 50,
            staked_only: false,
        }
    }
    fn udp_control(address: SocketAddr) -> Self {
        Self {
            protocol: "UDP",
            address,
            max_mbps: 10,
            staked_only: true,
        }
    }
    fn udp_bulk(address: SocketAddr) -> Self {
        Self {
            protocol: "UDP",
            address,
            max_mbps: 200,
            staked_only: true,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct AllowlistEntry {
    pub pubkey: Pubkey,
    pub stake: u64,
    pub gossip: Descriptor,
    pub serve_repair: Descriptor,
    pub tpu_forwards_quic: Option<Descriptor>,
    pub tpu_quic: Option<Descriptor>,
    pub tpu_vote: Option<Descriptor>,
    pub tpu_vote_quic: Option<Descriptor>,
    pub turbine: Descriptor,
    pub alpenglow: Option<Descriptor>,
}

impl AllowlistEntry {
    fn new(ports: &Ports, stake: u64) -> Self {
        Self {
            pubkey: ports.pubkey,
            stake,
            gossip: Descriptor::udp_gossip(ports.gossip),
            serve_repair: Descriptor::udp_bulk(ports.serve_repair),
            tpu_forwards_quic: ports.tpu_forwards_quic.map(Descriptor::quic),
            tpu_quic: ports.tpu_quic.map(Descriptor::quic),
            tpu_vote: ports.tpu_vote.map(Descriptor::udp_control),
            tpu_vote_quic: ports.tpu_vote_quic.map(Descriptor::quic),
            turbine: Descriptor::udp_bulk(ports.turbine),
            alpenglow: ports.alpenglow.map(Descriptor::udp_control),
        }
    }
}

async fn get_allowlist_full(State(state): State<AppState>) -> impl IntoResponse {
    let gossip = state.0.borrow();
    let stakes = state.1.borrow();
    let mut results = Vec::new();
    for (identity, ports) in gossip.gossip_data.iter() {
        let stake = stakes
            .stake_amounts
            .get(identity)
            .copied()
            .unwrap_or_default();
        results.push(AllowlistEntry::new(ports, stake));
    }
    Json(results)
}

#[derive(Deserialize)]
struct GetAllowlistShortParams {
    stake_threshold_sol: Option<u64>,
}

async fn get_allowlist_short(
    State(state): State<AppState>,
    Query(params): Query<GetAllowlistShortParams>,
) -> impl IntoResponse {
    let stake_threshold_lamports = params.stake_threshold_sol.unwrap_or(LAMPORTS_PER_SOL);
    #[derive(Debug, Default, Serialize)]
    struct Response {
        staked: Vec<Ipv4Addr>,
        unstaked: Vec<Ipv4Addr>,
    }
    let gossip = state.0.borrow();
    let stakes = state.1.borrow();
    let mut response = Response::default();
    for (identity, ports) in gossip.gossip_data.iter() {
        let stake = stakes
            .stake_amounts
            .get(identity)
            .copied()
            .unwrap_or_default();

        if stake > stake_threshold_lamports {
            response.staked.extend(ports.all_ips());
        } else {
            response.unstaked.extend(ports.all_ips());
        }
    }
    aggregate_into_24s(&mut response.staked);
    aggregate_into_24s(&mut response.unstaked);
    Json(response)
}

async fn get_health(State(state): State<AppState>) -> impl IntoResponse {
    let gossip_age = state.0.borrow().last_update.elapsed();
    let stake_age = state.1.borrow().last_update.elapsed();
    if gossip_age < Duration::from_secs(10) {
        Ok(format!(
            "gossip_age={gossip_age:?}\nstake_map_age={stake_age:?}\n"
        ))
    } else {
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

/// Aggregate IPv4 addresses into non-overlapping /24 subnets.
pub fn aggregate_into_24s(addrs: &mut Vec<Ipv4Addr>) {
    let mut nets = BTreeSet::new();

    for ip in addrs.iter() {
        let octets = ip.octets();
        // Zero the host bits → network address of /24
        nets.insert(Ipv4Addr::new(octets[0], octets[1], octets[2], 0));
    }
    addrs.clear();
    addrs.extend(nets);
}
