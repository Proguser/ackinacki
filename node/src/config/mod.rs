// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//
mod blockchain_config;
mod network_config;
mod serde_config;
#[cfg(test)]
mod test;
mod validations;
use std::path::PathBuf;
use std::time::Duration;

pub use blockchain_config::*;
pub use network_config::NetworkConfig;
use serde::Deserialize;
use serde::Serialize;
pub use serde_config::load_config_from_file;
pub use serde_config::save_config_to_file;
use typed_builder::TypedBuilder;

use crate::node::NodeIdentifier;
use crate::types::BlockSeqNo;

// TODO: These settings should be moved onchain.
/// Global node config, including block producer and synchronization settings.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GlobalConfig {
    /// Duration of one iteration of producing cycle in milliseconds.
    /// Defaults to 330
    pub time_to_produce_block_millis: u64,

    /// Maximum verification duration for one block.
    /// Defaults to 440 (330 * 4 / 3)
    pub time_to_verify_block_millis: u64,

    /// Maximum execution duration of one transaction production in milliseconds.
    /// Defaults to None
    pub time_to_produce_transaction_millis: Option<u64>,

    /// Maximum execution duration of one transaction verification in milliseconds.
    /// Defaults to None
    pub time_to_verify_transaction_millis: Option<u64>,

    /// Maximum execution duration of one transaction verification in milliseconds.
    /// Applied to transactions that was aborted with ExceptionCode::ExecutionTimeout.
    /// Defaults to None
    pub time_to_verify_transaction_aborted_with_execution_timeout_millis: Option<u64>,

    /// Timeout between attestation resend.
    pub attestation_resend_timeout: Duration,

    /// Difference between the seq no of the incoming block and the seq no of
    /// the last saved block, which causes the node synchronization process
    /// to start. Defaults to 20
    pub need_synchronization_block_diff: <BlockSeqNo as std::ops::Sub>::Output,

    /// Minimal time between publishing state.
    /// Defaults to 600 seconds
    pub min_time_between_state_publish_directives: Duration,

    /// Block gap size that causes block producer rotation.
    /// Defaults to 6
    pub producer_change_gap_size: usize,

    /// Timeout between consecutive NodeJoining messages sending.
    /// Defaults to 60 seconds
    pub node_joining_timeout: Duration,

    /// Block gap before sharing the state on sync.
    /// Defaults to 32
    pub sync_gap: u64,

    /// Delay in milliseconds which node waits after receiving block it can't
    /// apply before switching to sync mode.
    /// Defaults to 500
    pub sync_delay_milliseconds: u128,

    /// Save optimistic state frequency (every N'th block)
    /// Defaults to 200
    pub save_state_frequency: u32,

    /// Block keeper epoch code hash
    pub block_keeper_epoch_code_hash: String,

    /// Expected maximum number of threads.
    /// Note: it can grow over this value for some time on the running network.
    pub thread_count_soft_limit: usize,

    /// Thread load (aggregated number of messages in a queue to start splitting a thread) threshold for split
    pub thread_load_threshold: usize,

    /// Thread load window size, which is used to calculate thread load
    pub thread_load_window_size: usize,

    /// Change for a successfull attack
    pub chance_of_successful_attack: f64,
}

/// Node interaction settings
#[derive(Serialize, Deserialize, Debug, Clone, TypedBuilder)]
pub struct NodeConfig {
    /// Identifier of the current node.
    pub node_id: NodeIdentifier,

    /// Path to the file with blockchain config.
    #[builder(default = PathBuf::from("blockchain_config.json"))]
    pub blockchain_config_path: PathBuf,

    /// Path to the file with BLS key pair.
    #[builder(default = "block_keeper.keys.json".to_string())]
    pub key_path: String,

    /// Path to the file with block keeper seed key.
    #[builder(default = "block_keeper.keys.json".to_string())]
    pub block_keeper_seed_path: String,

    /// Path to zerostate file.
    #[builder(default = PathBuf::from("zerostate"))]
    pub zerostate_path: PathBuf,

    /// Local directory path which will be shared to other nodes.
    #[builder(default = PathBuf::from("/tmp"))]
    pub external_state_share_local_base_dir: PathBuf,

    /// Level of block production parallelization.
    #[builder(default = 20)]
    pub parallelization_level: usize,

    /// Store shard state and account BOCs separately.
    #[builder(default = false)]
    pub split_state: bool,

    /// Block cache size in local repository
    #[builder(default = 20)]
    pub block_cache_size: usize,

    /// State cache size in local repository
    #[builder(default = 10)]
    pub state_cache_size: usize,

    /// Path for message durable storage.
    #[builder(default = PathBuf::from("./message_storage/db"))]
    pub message_storage_path: PathBuf,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    /// Global config
    #[serde(default)]
    pub global: GlobalConfig,

    /// Network config
    pub network: NetworkConfig,

    /// Local config
    pub local: NodeConfig,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            time_to_produce_block_millis: 330,
            time_to_verify_block_millis: 330 * 4 / 3,
            time_to_produce_transaction_millis: None,
            time_to_verify_transaction_millis: None,
            time_to_verify_transaction_aborted_with_execution_timeout_millis: None,
            need_synchronization_block_diff: 20,
            min_time_between_state_publish_directives: Duration::from_secs(600),
            attestation_resend_timeout: Duration::from_secs(3),
            producer_change_gap_size: 6,
            node_joining_timeout: Duration::from_secs(300),
            sync_gap: 32,
            sync_delay_milliseconds: 500,
            // TODO: Critical! Fix repo issue and revert the value back to 200
            save_state_frequency: 200,
            block_keeper_epoch_code_hash:
                "8246c7bdd8f2559b5f00e4334dba4612c2f48f52f0e3a5390298543d51a1ff1e".to_string(),
            thread_count_soft_limit: 100,
            thread_load_window_size: 100,
            thread_load_threshold: 5000,
            chance_of_successful_attack: 0.000000001_f64,
        }
    }
}

#[cfg(test)]
impl Default for NodeConfig {
    fn default() -> Self {
        NodeConfig {
            node_id: NodeIdentifier::some_id(),
            blockchain_config_path: PathBuf::from("blockchain_config.json"),
            key_path: "block_keeper.keys.json".to_string(),
            zerostate_path: PathBuf::from("zerostate"),
            external_state_share_local_base_dir: PathBuf::from("/tmp"),
            parallelization_level: 20,
            block_keeper_seed_path: "block_keeper.keys.json".to_string(),
            split_state: false,
            block_cache_size: 20,
            state_cache_size: 10,
            message_storage_path: PathBuf::from("./message_storage/db"),
        }
    }
}

pub fn must_save_state_on_seq_no(
    seq_no: BlockSeqNo,
    parent_seq_no: Option<BlockSeqNo>,
    save_state_frequency: u32,
) -> bool {
    let seq_no = u32::from(seq_no);
    if let Some(parent_seq_no) = parent_seq_no {
        (u32::from(parent_seq_no) / save_state_frequency) != (seq_no / save_state_frequency)
    } else {
        seq_no % save_state_frequency == 0
    }
}
