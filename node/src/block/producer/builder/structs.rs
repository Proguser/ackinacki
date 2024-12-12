// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;

use serde::Serialize;
use tvm_block::Block;
use tvm_block::BlockInfo;
use tvm_block::CopyleftRewards;
use tvm_block::CurrencyCollection;
use tvm_block::Grams;
use tvm_block::InMsgDescr;
use tvm_block::Message;
use tvm_block::OutMsgDescr;
use tvm_block::OutMsgQueueInfo;
use tvm_block::ShardAccountBlocks;
use tvm_block::ShardAccounts;
use tvm_block::ShardStateUnsplit;
use tvm_block::StateInit;
use tvm_block::Transaction;
use tvm_types::AccountId;
use tvm_types::Cell;
use tvm_types::UInt256;
use tvm_types::UsageTree;

use crate::block_keeper_system::BlockKeeperSetChange;
use crate::config::Config;
use crate::creditconfig::DappConfig;
use crate::repository::optimistic_state::OptimisticStateImpl;
use crate::types::AccountAddress;
use crate::types::BlockEndLT;
use crate::types::DAppIdentifier;

pub struct PreparedBlock {
    pub block: Block,
    pub remain_fees: Grams,
    pub state: OptimisticStateImpl,
    pub is_empty: bool,
    // TODO: check if we can get rid of it
    pub transaction_traces: HashMap<UInt256, Vec<EngineTraceInfoData>>,
    pub active_threads: Vec<(Cell, ActiveThread)>,
    pub tx_cnt: usize,
    pub block_keeper_set_changes: Vec<BlockKeeperSetChange>,
}

pub struct ThreadResult {
    pub transaction: Transaction,
    pub lt: u64,
    pub trace: Option<Vec<EngineTraceInfoData>>,
    pub account_root: Cell,
    pub account_id: AccountId,
    pub minted_shell: u128,
    pub dapp_id: Option<UInt256>,
}

pub struct ActiveThread {
    pub thread: JoinHandle<anyhow::Result<ThreadResult>>,
    pub message: Message,
    pub vm_execution_is_block_related: Arc<Mutex<bool>>,
    pub block_production_was_finished: Arc<Mutex<bool>>,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct EngineTraceInfoData {
    pub info_type: String,
    pub step: u32, // number of executable command
    pub cmd_str: String,
    pub stack: Vec<String>,
    pub gas_used: String,
    pub gas_cmd: String,
    pub cmd_code_rem_bits: u32,
    pub cmd_code_hex: String,
    pub cmd_code_cell_hash: String,
    pub cmd_code_offset: u32,
}

type MessageIndex = u128;

/// BlockBuilder structure
pub struct BlockBuilder {
    pub(crate) shard_state: Arc<ShardStateUnsplit>,
    pub(crate) accounts: ShardAccounts,
    pub(crate) block_info: BlockInfo,
    pub(crate) rand_seed: UInt256,
    pub(crate) new_messages: BTreeMap<MessageIndex, (Message, Option<Cell>)>, /* Mapping of messages generated
                                                                               * while execution */
    pub(crate) from_prev_blk: CurrencyCollection,
    pub(crate) in_msg_descr: InMsgDescr,
    pub(crate) out_msg_descr: OutMsgDescr,
    pub(crate) out_queue_info: OutMsgQueueInfo,
    pub(crate) block_gas_limit: u64,
    pub(crate) account_blocks: ShardAccountBlocks,
    pub(crate) total_gas_used: u64,
    pub(crate) total_message_processed: usize,
    pub(crate) start_lt: u64,
    pub(crate) end_lt: u64, // biggest logical time of all messages
    pub(crate) copyleft_rewards: CopyleftRewards,
    pub(crate) transaction_traces: HashMap<UInt256, Vec<EngineTraceInfoData>>,

    pub(crate) control_rx_stop: Option<Receiver<()>>,
    pub(crate) parallelization_level: usize,
    pub(crate) tx_cnt: usize,

    pub(crate) usage_tree: UsageTree,
    pub(crate) initial_optimistic_state: OptimisticStateImpl,
    pub(crate) node_config: Config,
    pub(crate) block_keeper_set_changes: Vec<BlockKeeperSetChange>,
    pub(crate) base_config_stateinit: StateInit,
    pub(crate) dapp_credit_map: HashMap<UInt256, DappConfig>,
    pub(crate) dapp_minted_map: HashMap<UInt256, u128>,
    pub(crate) dapp_id_table: HashMap<AccountAddress, (Option<DAppIdentifier>, BlockEndLT)>,
}
