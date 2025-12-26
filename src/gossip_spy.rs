use crate::GossipData;
use anyhow::Context;
use log::info;
use serde::{Deserialize, Serialize};
use solana_gossip::contact_info::ContactInfo;
use solana_gossip::gossip_service::make_gossip_node;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_streamer::socket::SocketAddrSpace;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::{net::SocketAddr, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ports {
    pub pubkey: Pubkey,
    pub stake: u64,
    pub gossip: SocketAddr,
    // pub repair: Option<SocketAddr>, TODO : figure out what to do with this one
    pub serve_repair: SocketAddr,
    pub tpu_forwards_quic: Option<SocketAddr>,
    pub tpu_quic: Option<SocketAddr>,
    pub tpu_vote: Option<SocketAddr>,
    pub tpu_vote_quic: Option<SocketAddr>,
    pub turbine: SocketAddr,
    pub alpenglow: Option<SocketAddr>,
}

impl Ports {
    pub fn all_ips(&self) -> Vec<Ipv4Addr> {
        let mut ips = vec![self.gossip.ip(), self.serve_repair.ip(), self.turbine.ip()];

        if let Some(addr) = self.tpu_forwards_quic {
            ips.push(addr.ip());
        }
        if let Some(addr) = self.tpu_quic {
            ips.push(addr.ip());
        }
        if let Some(addr) = self.tpu_vote {
            ips.push(addr.ip());
        }
        if let Some(addr) = self.tpu_vote_quic {
            ips.push(addr.ip());
        }
        if let Some(addr) = self.alpenglow {
            ips.push(addr.ip());
        }

        ips.iter()
            .filter_map(|ip| {
                if let IpAddr::V4(a) = ip {
                    Some(*a)
                } else {
                    None
                }
            })
            .collect()
    }
}

impl TryFrom<ContactInfo> for Ports {
    type Error = anyhow::Error;
    fn try_from(value: ContactInfo) -> anyhow::Result<Self> {
        use solana_gossip::contact_info::Protocol::{QUIC, UDP};
        let p = Ports {
            stake: 0,
            pubkey: *value.pubkey(),
            gossip: value.gossip().context("gossip is required")?,
            serve_repair: value.serve_repair(UDP).context("server repair required")?,
            tpu_forwards_quic: value.tpu_forwards(QUIC),
            tpu_quic: value.tpu(QUIC),
            tpu_vote: value.tpu_vote(UDP),
            tpu_vote_quic: value.tpu_vote(QUIC),
            turbine: value.tvu(UDP).context("TVU is required")?,
            alpenglow: value.alpenglow(),
        };
        Ok(p)
    }
}

pub fn get_shred_version(entrypoint: SocketAddr) -> anyhow::Result<u16> {
    let shred_version =
        solana_net_utils::get_cluster_shred_version(&entrypoint).map_err(|s| anyhow::anyhow!(s))?;
    info!("Cluster's shred version is {}", &shred_version);
    Ok(shred_version)
}

/// Finds the turbine port on a given gossip endpoint
pub async fn watch_gossip(
    my_gossip_addr: SocketAddr,
    entrypoint: SocketAddr,
    shred_version: u16,
    cancel: CancellationToken,
    gossip_data_tx: tokio::sync::watch::Sender<GossipData>,
) -> anyhow::Result<()> {
    let keypair = Keypair::new();
    let exit = Arc::new(AtomicBool::new(false));
    let (_gossip_service, _ip_echo, spy_ref) = make_gossip_node(
        keypair,
        Some(&entrypoint),
        exit.clone(),
        Some(&my_gossip_addr),
        shred_version,
        true, // should_check_duplicate_instance,
        SocketAddrSpace::new(true),
    );
    while !cancel.is_cancelled() {
        let gossip_data = spy_ref
            .all_peers()
            .into_iter()
            .filter_map(|(x, _)| Ports::try_from(x).ok())
            .map(|x| (x.pubkey, x))
            .collect::<HashMap<_, _>>();
        info!("Gossip table has {} entries", gossip_data.len());
        let value = GossipData {
            gossip_data,
            ..Default::default()
        };
        gossip_data_tx.send(value)?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}
