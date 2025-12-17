use crate::GossipData;
use anyhow::Context;
use log::info;

use serde::{Deserialize, Serialize};
use solana_gossip::contact_info::ContactInfo;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_streamer::socket::SocketAddrSpace;
use std::collections::HashMap;
use std::io::{BufReader, prelude::*};
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::{net::SocketAddr, time::Duration};
use tokio_util::sync::CancellationToken;

use solana_gossip::gossip_service::make_gossip_node;

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

        // Optional fields
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
        tokio::time::sleep(Duration::from_secs(10)).await;
    }

    // let turbine = me.tvu(solana_gossip::contact_info::Protocol::UDP);
    // let serve_repair = me.serve_repair(solana_gossip::contact_info::Protocol::UDP);

    // let tpu = me.tpu(solana_gossip::contact_info::Protocol::UDP);
    // let tpu_quic = me.tpu(solana_gossip::contact_info::Protocol::QUIC);
    // let tpu_vote = me.tpu_vote(solana_gossip::contact_info::Protocol::UDP);
    // let tpu_vote_quic = me.tpu_vote(solana_gossip::contact_info::Protocol::QUIC);

    Ok(())
}

pub(crate) fn _get_leader_schedule() -> anyhow::Result<HashMap<u64, Pubkey>> {
    info!("Fetching leader schedule");
    let child = std::process::Command::new("solana")
        .args(["-um", "leader-schedule"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("Could not call solana cli")?;
    let output = child
        .wait_with_output()
        .context("wait for leader schedule to be fetched")?;
    let reader: BufReader<_> = std::io::BufReader::new(std::io::Cursor::new(output.stdout));
    let mut schedule = HashMap::with_capacity(2000);
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (slot, key) = line
            .split_once(' ')
            .ok_or_else(|| anyhow::anyhow!("invalid format"))?;
        let slot: u64 = slot.trim().parse()?;
        let key = key.trim();
        let key = Pubkey::from_str(key)?;
        schedule.insert(slot, key);
    }
    Ok(schedule)
}

/*
pub fn process_leader_schedule(
    rpc_client: &RpcClient,
    config: &CliConfig,
    epoch: Option<Epoch>,
) -> ProcessResult {
    let epoch_info = rpc_client.get_epoch_info()?;
    let epoch = epoch.unwrap_or(epoch_info.epoch);
    if epoch > epoch_info.epoch.saturating_add(1) {
        return Err(format!("Epoch {epoch} is more than one epoch in the future").into());
    }

    let epoch_schedule = rpc_client.get_epoch_schedule()?;
    let first_slot_in_epoch = epoch_schedule.get_first_slot_in_epoch(epoch);

    let leader_schedule = rpc_client.get_leader_schedule(Some(first_slot_in_epoch))?;
    if leader_schedule.is_none() {
        return Err(
            format!("Unable to fetch leader schedule for slot {first_slot_in_epoch}").into(),
        );
    }
    let leader_schedule = leader_schedule.unwrap();

    let mut leader_per_slot_index = Vec::new();
    for (pubkey, leader_slots) in leader_schedule.iter() {
        for slot_index in leader_slots.iter() {
            if *slot_index >= leader_per_slot_index.len() {
                leader_per_slot_index.resize(slot_index.saturating_add(1), "?");
            }
            leader_per_slot_index[*slot_index] = pubkey;
        }
    }

    let mut leader_schedule_entries = vec![];
    for (slot_index, leader) in leader_per_slot_index.iter().enumerate() {
        leader_schedule_entries.push(CliLeaderScheduleEntry {
            slot: first_slot_in_epoch.saturating_add(slot_index as u64),
            leader: leader.to_string(),
        });
    }

    Ok(config.output_format.formatted_string(&CliLeaderSchedule {
        epoch,
        leader_schedule_entries,
    }))
}
*/
