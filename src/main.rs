use std::{net::SocketAddr, time::Duration};

use crate::{
    gossip_spy::{get_shred_version, watch_gossip},
    http_endpoint::run_http,
    ranking_table::{GossipData, StakeData},
    rpc_request::stakes_refresh_loop,
};
use clap::Parser;
use solana_net_utils::parse_host_port;
use tokio::signal::ctrl_c;
use tokio_util::sync::CancellationToken;

mod gossip_spy;
mod http_endpoint;
mod ranking_table;
mod rpc_request;

#[derive(Parser)]
#[command(version, about,  long_about = None)]
struct Cli {
    #[arg(short, long)]
    verbose: bool,
    #[arg(short, long, value_parser=parse_host_port)]
    known_gossip_peer: SocketAddr,
    #[arg(short, long)]
    rpc_url: String,
    #[arg(short, long)]
    api_bind_address: SocketAddr,
    #[arg(short, long)]
    gossip_spy_bind_address: SocketAddr,
    #[arg(short, long, default_value = "300")]
    /// Timeout for discovery of turbine and repair ports. set to 0 to ony work with gossip
    discovery_timeout_sec: u64,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    solana_logger::setup_with("info,solana_metrics=error,solana_gossip=warn");
    let cli = Cli::parse();

    let shred_version =
        tokio::task::spawn_blocking(move || get_shred_version(cli.known_gossip_peer)).await??;
    let cancel = CancellationToken::new();

    let (stakes_tx, stakes_watch) = tokio::sync::watch::channel(StakeData::default());

    let rpc_watch = stakes_refresh_loop(cli.rpc_url, None, stakes_tx, Duration::from_secs(60));

    let (gossip_data_tx, gossip_data_watch) = tokio::sync::watch::channel(GossipData::default());

    let gossip_watch = tokio::spawn(watch_gossip(
        cli.gossip_spy_bind_address,
        cli.known_gossip_peer,
        shred_version,
        cancel.child_token(),
        gossip_data_tx,
    ));

    let http_server = run_http((gossip_data_watch, stakes_watch), cli.api_bind_address);
    tokio::select!(
        _ =ctrl_c() => {println!("Received SIGINT, exiting");},
        v= rpc_watch=>{eprintln!("{v:?}");}
        v= gossip_watch=>{eprintln!("{v:?}");},
        v= http_server=>{eprintln!("{v:?}");}
    );
    cancel.cancel();
    std::process::exit(0);
}
