#![allow(dead_code)]

mod agent;
mod network;
mod storage;

pub use agent::*;
pub use network::*;
pub use storage::*;

use anyhow::{anyhow, Result};
use async_raft::config::Config;
use async_raft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    Raft, VoteRequest, VoteResponse,
};
use async_raft::NodeId;
use bld_config::BldConfig;
use bld_utils::sync::IntoArc;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sqlite::SqliteConnection;
use std::collections::HashSet;

pub type HighAvailRaft = Raft<AgentRequest, AgentResponse, HighAvailRouter, HighAvailStore>;

pub enum HighAvailThreadRequest {
    AppendEntries(AppendEntriesRequest<AgentRequest>),
    InstallSnapshot(InstallSnapshotRequest),
    Vote(VoteRequest),
}

pub enum HighAvailThreadResponse {
    AppendEntries(AppendEntriesResponse),
    InstallSnapshot(InstallSnapshotResponse),
    Vote(VoteResponse),
}

pub struct HighAvailThread {
    node_id: NodeId,
    raft: HighAvailRaft,
}

impl HighAvailThread {
    pub async fn new(
        config: &BldConfig,
        pool: Pool<ConnectionManager<SqliteConnection>>,
    ) -> Result<Self> {
        let (agent, agents) = agent_info(config)?;
        let node_id = agent.id();
        let raft_config = Config::build("raft-group".into()).validate()?.into_arc();
        let ids = agents.iter().map(|a| a.id()).collect::<HashSet<NodeId>>();
        let network = HighAvailRouter::new(raft_config.clone(), agents)
            .await?
            .into_arc();
        let store = HighAvailStore::new(pool, agent.id())?.into_arc();
        let raft = HighAvailRaft::new(agent.id(), raft_config, network, store);
        raft.initialize(ids).await.map_err(|e| anyhow!(e))?;
        Ok(Self { node_id, raft })
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn raft(&self) -> &HighAvailRaft {
        &self.raft
    }
}

fn agent_info(config: &BldConfig) -> Result<(Agent, HashSet<Agent>)> {
    let node_id = config
        .local
        .node_id
        .ok_or_else(|| anyhow!("node_id not found"))?;
    let server = &config.local.server;
    let agent = Agent::new(node_id, &server.host, server.port, &server.http_protocol());
    let mut agents = HashSet::new();
    for server in config.remote.servers.iter() {
        let node_id = server
            .node_id
            .ok_or_else(|| anyhow!("server: {}, node_id not found", server.name))?;
        agents.insert(Agent::new(
            node_id,
            &server.host,
            server.port,
            &server.http_protocol(),
        ));
    }
    agents.insert(agent.clone());
    Ok((agent, agents))
}

pub enum HighAvail {
    Enabled(HighAvailThread),
    Disabled,
}

impl HighAvail {
    pub async fn new(
        config: &BldConfig,
        pool: Pool<ConnectionManager<SqliteConnection>>,
    ) -> Result<Self> {
        Ok(match config.local.ha_mode {
            true => Self::Enabled(HighAvailThread::new(config, pool).await?),
            false => Self::Disabled,
        })
    }
}
