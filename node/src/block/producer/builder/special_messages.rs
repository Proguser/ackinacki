// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;

use tvm_block::Augmentation;
use tvm_block::EnqueuedMsg;
use tvm_block::GetRepresentationHash;
use tvm_block::HashmapAugType;
use tvm_block::Message;
use tvm_block::MsgEnvelope;
use tvm_block::OutMsg;
use tvm_block::Serializable;
use tvm_executor::BlockchainConfig;
use tvm_types::UInt256;

use super::BlockBuilder;
use crate::block::producer::execution_time::ExecutionTimeLimits;
use crate::block_keeper_system::epoch::create_epoch_touch_message;
use crate::block_keeper_system::BlockKeeperData;
use crate::creditconfig::dappconfig::calculate_dapp_config_address;
use crate::creditconfig::dappconfig::create_config_touch_message;
use crate::message::identifier::MessageIdentifier;
use crate::message::WrappedMessage;
use crate::repository::optimistic_state::OptimisticState;
use crate::types::AccountAddress;

impl BlockBuilder {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_epoch_messages(
        &mut self,
        epoch_block_keeper_data: &mut Vec<BlockKeeperData>,
        blockchain_config: &BlockchainConfig,
        block_unixtime: u32,
        block_lt: u64,
        check_messages_map: &Option<HashMap<UInt256, u64>>,
    ) -> anyhow::Result<()> {
        // execute epoch messages
        tracing::trace!(
            "execute_epoch_messages: epoch_block_keeper_data={epoch_block_keeper_data:?}"
        );
        let mut active_destinations = HashSet::new();
        let mut active_ext_threads = VecDeque::new();
        loop {
            // If active pool is not full add threads
            if active_ext_threads.len() < self.parallelization_level {
                while !epoch_block_keeper_data.is_empty() {
                    if active_ext_threads.len() == self.parallelization_level {
                        break;
                    }
                    let msg = create_epoch_touch_message(
                        &epoch_block_keeper_data[0],
                        self.block_info.gen_utime().as_u32(),
                    )?;
                    if let Some(acc_id) = msg.int_dst_account_id() {
                        if !self
                            .initial_optimistic_state
                            .does_account_belong_to_the_state(&acc_id)?
                        {
                            epoch_block_keeper_data.remove(0);
                            tracing::warn!(
                                target: "builder",
                                "Epoch msg destination does not belong to the current thread: {:?}",
                                msg
                            );
                        } else if !active_destinations.contains(&acc_id) {
                            epoch_block_keeper_data.remove(0);
                            tracing::trace!(target: "builder", "Parallel epoch message: {:?} to {:?}", msg.hash().unwrap(), acc_id.to_hex_string());
                            let thread = self.execute(
                                msg,
                                blockchain_config,
                                &acc_id,
                                block_unixtime,
                                block_lt,
                                check_messages_map,
                                &ExecutionTimeLimits::NO_LIMITS,
                            )?;
                            active_ext_threads.push_back(thread);
                            active_destinations.insert(acc_id);
                        } else {
                            break;
                        }
                    } else {
                        epoch_block_keeper_data.remove(0);
                        tracing::warn!(
                            target: "builder",
                            "Found epoch msg with not valid internal destination: {:?}",
                            msg
                        );
                    }
                }
            }

            while !active_ext_threads.is_empty() {
                if let Ok(thread_result) = active_ext_threads.front().unwrap().result_rx.try_recv()
                {
                    let _ = active_ext_threads.pop_front().unwrap();
                    let thread_result = thread_result.map_err(|_| {
                        anyhow::format_err!("Failed to execute transaction in parallel")
                    })?;
                    let acc_id = thread_result.account_id.clone();
                    tracing::trace!(target: "builder", "parallel epoch message finished dest: {}", acc_id.to_hex_string());
                    self.after_transaction(thread_result)?;
                    active_destinations.remove(&acc_id);
                } else {
                    break;
                }
            }

            if epoch_block_keeper_data.is_empty() && active_ext_threads.is_empty() {
                tracing::debug!(target: "builder", "Epoch messages stop");
                break;
            }
        }
        Ok(())
    }

    pub(super) fn execute_dapp_config_messages(
        &mut self,
        blockchain_config: &BlockchainConfig,
        block_unixtime: u32,
        block_lt: u64,
        check_messages_map: &Option<HashMap<UInt256, u64>>,
    ) -> anyhow::Result<()> {
        tracing::trace!(target: "builder", "map of minted shell {:?}", self.dapp_minted_map);
        let mut config_messages: Vec<Message> = Vec::new();
        // execute DappConfig messages
        for (key, value) in self.dapp_minted_map.clone() {
            if value == 0 {
                continue;
            }
            let data = self.dapp_credit_map.get(&key);
            if let Some(configdata) = data {
                if configdata.is_unlimit {
                    continue;
                }
                let addr =
                    calculate_dapp_config_address(key.clone(), self.base_config_stateinit.clone())
                        .map_err(|e| {
                            anyhow::format_err!("Failed to calculate dapp config address: {e}")
                        })?;
                let message =
                    create_config_touch_message(value, addr, self.block_info.gen_utime().as_u32())
                        .map_err(|e| {
                            anyhow::format_err!("Failed to create config touch message: {e}")
                        })?;

                let wrapped_message = WrappedMessage { message: message.clone() };
                tracing::debug!(
                    "Add config message to produced_internal_messages_to_the_current_thread: {}",
                    wrapped_message.message.hash().unwrap().to_hex_string()
                );
                let entry = self
                    .produced_internal_messages_to_the_current_thread
                    .entry(AccountAddress(message.dst().expect("must be set").address().clone()))
                    .or_default();
                entry.push((MessageIdentifier::from(&wrapped_message), Arc::new(wrapped_message)));

                let info = message.int_header().unwrap();
                let fwd_fee = info.fwd_fee();
                let msg_cell = message.serialize().map_err(|e| {
                    anyhow::format_err!("Failed to serialize dapp config message: {e}")
                })?;
                let env = MsgEnvelope::with_message_and_fee(&message, *fwd_fee).map_err(|e| {
                    anyhow::format_err!("Failed to generate dapp config message envelope: {e}")
                })?;
                let enq = EnqueuedMsg::with_param(info.created_lt, &env).map_err(|e| {
                    anyhow::format_err!("Failed to enqueue dapp config message: {e}")
                })?;
                // Note: this message was generate by node and has no parent transaction, use default tx instead
                let default_tx = tvm_block::Transaction::default().serialize().map_err(|e| {
                    anyhow::format_err!("Failed to serialize default transaction: {e}")
                })?;
                let out_msg = OutMsg::new(enq.out_msg_cell(), default_tx);

                self.out_msg_descr
                    .set(
                        &msg_cell.repr_hash(),
                        &out_msg,
                        &out_msg
                            .aug()
                            .map_err(|e| anyhow::format_err!("Failed to get msg aug: {e}"))?,
                    )
                    .map_err(|e| anyhow::format_err!("Failed to add msg to out msg descr: {e}"))?;
                config_messages.push(message);
            }
        }
        let mut active_destinations = HashSet::new();
        let mut active_ext_threads = VecDeque::new();
        loop {
            if active_ext_threads.len() < self.parallelization_level {
                while !config_messages.is_empty() {
                    if active_ext_threads.len() == self.parallelization_level {
                        break;
                    }
                    if let Some(acc_id) = config_messages[0].int_dst_account_id() {
                        if !active_destinations.contains(&acc_id) {
                            let msg = config_messages.remove(0);
                            tracing::trace!(target: "builder", "Parallel config message: {:?} to {:?}", msg.hash().unwrap(), acc_id.to_hex_string());
                            let thread = self.execute(
                                msg,
                                blockchain_config,
                                &acc_id,
                                block_unixtime,
                                block_lt,
                                check_messages_map,
                                &ExecutionTimeLimits::NO_LIMITS,
                            )?;
                            active_ext_threads.push_back(thread);
                            active_destinations.insert(acc_id);
                        } else {
                            break;
                        }
                    } else {
                        tracing::warn!(
                            target: "builder",
                            "Found dapp config msg with not valid internal destination: {:?}",
                            config_messages.remove(0)
                        );
                    }
                }
            }

            while !active_ext_threads.is_empty() {
                if let Ok(thread_result) = active_ext_threads.front().unwrap().result_rx.try_recv()
                {
                    let _ = active_ext_threads.pop_front().unwrap();
                    let thread_result = thread_result.map_err(|_| {
                        anyhow::format_err!("Failed to execute transaction in parallel")
                    })?;
                    let acc_id = thread_result.account_id.clone();
                    tracing::trace!(target: "builder", "parallel epoch message finished dest: {}", acc_id.to_hex_string());
                    self.after_transaction(thread_result)?;
                    active_destinations.remove(&acc_id);
                } else {
                    break;
                }
            }

            if config_messages.is_empty() && active_ext_threads.is_empty() {
                tracing::debug!(target: "builder", "Dapp Config messages stop");
                break;
            }
        }
        Ok(())
    }
}
