// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use std::collections::HashMap;
use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;
use std::thread::sleep;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use http_server::ExtMsgFeedbackList;
use parking_lot::Mutex;
use telemetry_utils::mpsc::instrumented_channel;
use telemetry_utils::mpsc::InstrumentedReceiver;
use telemetry_utils::mpsc::InstrumentedSender;
use tracing::instrument;
use tracing::trace_span;
use tvm_block::Message;
use tvm_block::ShardStateUnsplit;
use tvm_executor::BlockchainConfig;
use tvm_types::Cell;
use tvm_types::UInt256;
use typed_builder::TypedBuilder;

use crate::block::producer::builder::ActiveThread;
use crate::block::producer::execution_time::ExecutionTimeLimits;
use crate::block::producer::execution_time::ProductionTimeoutCorrection;
use crate::block::producer::BlockProducer;
use crate::block::producer::TVMBlockProducer;
use crate::block_keeper_system::BlockKeeperData;
use crate::bls::envelope::BLSSignedEnvelope;
use crate::bls::envelope::Envelope;
use crate::bls::BLSSignatureScheme;
use crate::bls::GoshBLS;
use crate::config::Config;
use crate::external_messages::ExternalMessagesThreadState;
use crate::helper::block_flow_trace;
use crate::helper::block_flow_trace_with_time;
use crate::helper::metrics::BlockProductionMetrics;
use crate::multithreading::cross_thread_messaging::thread_references_state::CanRefQueryResult;
use crate::node::associated_types::AckData;
use crate::node::associated_types::NackData;
use crate::node::block_state::repository::BlockStateRepository;
use crate::node::shared_services::SharedServices;
use crate::node::NodeIdentifier;
use crate::repository::accounts::AccountsRepository;
use crate::repository::cross_thread_ref_repository::CrossThreadRefDataRead;
use crate::repository::optimistic_state::OptimisticState;
use crate::repository::optimistic_state::OptimisticStateImpl;
use crate::repository::repository_impl::RepositoryImpl;
use crate::repository::Repository;
use crate::types::AckiNackiBlock;
use crate::types::BlockIdentifier;
use crate::types::ThreadIdentifier;
use crate::utilities::FixedSizeHashSet;

#[derive(TypedBuilder)]
pub struct TVMBlockProducerProcess {
    node_config: Config,
    blockchain_config: Arc<BlockchainConfig>,
    repository: RepositoryImpl,
    archive_sender: std::sync::mpsc::Sender<(
        Envelope<GoshBLS, AckiNackiBlock>,
        Option<Arc<ShardStateUnsplit>>,
        Option<Cell>,
    )>,
    #[builder(default)]
    produced_blocks: Arc<Mutex<Vec<(AckiNackiBlock, OptimisticStateImpl, ExtMsgFeedbackList)>>>,
    #[builder(default)]
    active_producer_thread: Option<(JoinHandle<OptimisticStateImpl>, InstrumentedSender<()>)>,
    block_produce_timeout: Arc<Mutex<Duration>>,
    #[builder(default)]
    optimistic_state_cache: Option<OptimisticStateImpl>,
    #[builder(default)]
    epoch_block_keeper_data_senders: Option<InstrumentedSender<BlockKeeperData>>,
    shared_services: SharedServices,

    producer_node_id: NodeIdentifier,
    thread_count_soft_limit: usize,
    parallelization_level: usize,
    block_keeper_epoch_code_hash: String,
    metrics: Option<BlockProductionMetrics>,
}

impl TVMBlockProducerProcess {
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all)]
    fn produce_next(
        node_config: Config,
        initial_state: &mut OptimisticStateImpl,
        blockchain_config: Arc<BlockchainConfig>,
        producer_node_id: NodeIdentifier,
        thread_count_soft_limit: usize,
        parallelization_level: usize,
        block_keeper_epoch_code_hash: String,
        produced_blocks: Arc<Mutex<Vec<(AckiNackiBlock, OptimisticStateImpl, ExtMsgFeedbackList)>>>,
        timeout: Arc<Mutex<Duration>>,
        timeout_correction: &mut ProductionTimeoutCorrection,
        thread_id_clone: ThreadIdentifier,
        epoch_block_keeper_data_rx: &InstrumentedReceiver<BlockKeeperData>,
        shared_services: &mut SharedServices,
        active_block_producer_threads: &mut Vec<(Cell, ActiveThread)>,
        received_acks: Arc<Mutex<Vec<Envelope<GoshBLS, AckData>>>>,
        received_nacks: Arc<Mutex<Vec<Envelope<GoshBLS, NackData>>>>,
        block_state_repo: BlockStateRepository,
        accounts_repo: AccountsRepository,
        external_control_rx: &InstrumentedReceiver<()>,
        metrics: Option<BlockProductionMetrics>,
        external_messages_queue: &ExternalMessagesThreadState,
        repository: &RepositoryImpl,
    ) -> bool {
        tracing::trace!("Start block production process iteration");
        let start_time = std::time::SystemTime::now();
        let production_time = Instant::now();
        let (
            message_queue,
            epoch_block_keeper_data,
            block_nack,
            aggregated_acks,
            aggregated_nacks,
            initial_external_messages_progress,
        ) = trace_span!("read messages").in_scope(|| {
            let messages = external_messages_queue
                .get_remaining_external_messages(&initial_state.block_id)
                .expect("Must work");
            let message_queue: VecDeque<Message> =
                messages.clone().into_iter().map(|wrap| wrap.message).collect();
            let mut received_acks_in = received_acks.lock();
            let received_acks_copy = received_acks_in.clone();
            received_acks_in.clear();
            drop(received_acks_in);
            // TODO: filter that nacks are not older than last finalized block
            let mut received_nacks_in = received_nacks.lock();
            let received_nacks_copy = received_nacks_in.clone();
            received_nacks_in.clear();
            drop(received_nacks_in);
            let aggregated_acks =
                aggregate_acks(received_acks_copy).expect("Failed to aggregate acks");
            let aggregated_nacks =
                aggregate_nacks(received_nacks_copy).expect("Failed to aggregate nacks");
            let block_nack = aggregated_nacks.clone();
            let mut epoch_block_keeper_data = vec![];
            while let Ok(data) = epoch_block_keeper_data_rx.try_recv() {
                tracing::trace!("Received data for epoch: {data:?}");
                epoch_block_keeper_data.push(data);
            }

            tracing::Span::current().record("messages.len", messages.len());
            (
                message_queue,
                epoch_block_keeper_data,
                block_nack,
                aggregated_acks,
                aggregated_nacks,
                external_messages_queue
                    .get_progress(&initial_state.block_id)
                    .expect("Must not fail")
                    .expect("Parent block external messages processing progress must be stored"),
            )
        });

        let producer = TVMBlockProducer::builder()
            .active_threads(mem::take(active_block_producer_threads))
            .blockchain_config(blockchain_config.clone())
            .message_queue(message_queue)
            .producer_node_id(producer_node_id.clone())
            .thread_count_soft_limit(thread_count_soft_limit)
            .parallelization_level(parallelization_level)
            .block_keeper_epoch_code_hash(block_keeper_epoch_code_hash)
            .epoch_block_keeper_data(epoch_block_keeper_data)
            .shared_services(shared_services.clone())
            .block_nack(block_nack.clone())
            .accounts(accounts_repo)
            .block_state_repository(block_state_repo)
            .metrics(metrics.clone())
            .build();

        let (control_tx, control_rx) =
            instrumented_channel(metrics.clone(), crate::helper::metrics::ROUTING_COMMAND_CHANNEL);

        let initial_state_clone = initial_state.clone();
        tracing::trace!(
            "produce_block refs: initial state ThreadReferencesState: {:?}",
            initial_state_clone.thread_refs_state
        );

        let refs = trace_span!("read thread refs").in_scope(|| {
            let buffer: Vec<BlockIdentifier> =
                trace_span!("guarded list_blocks_sending_messages_to_thread").in_scope(|| {
                    shared_services
                        .exec(|services| {
                            services
                                .thread_sync
                                .list_blocks_sending_messages_to_thread(&thread_id_clone)
                        })
                        .expect("Failed to list potential ref block")
                });

            let can_ref_query_result = trace_span!("bulk can_reference query").in_scope(|| {
                shared_services
                    .exec(|e| {
                        initial_state_clone.thread_refs_state.can_reference(
                            buffer.clone(),
                            |block_id| {
                                e.cross_thread_ref_data_service.get_cross_thread_ref_data(block_id)
                            },
                        )
                    })
                    .expect("Can query block must work")
            });

            let mut refs = vec![];
            if let CanRefQueryResult::Yes(result_state) = can_ref_query_result {
                refs.extend(result_state.explicitly_referenced_blocks);
            }
            // TODO: reuse previously loaded cross thread ref data
            let refs = trace_span!("get_cross_thread_ref_data for refs").in_scope(|| {
                shared_services.exec(|e| {
                    refs.into_iter()
                        .map(|r| {
                            e.cross_thread_ref_data_service
                                .get_cross_thread_ref_data(&r)
                                .expect("Failed to load ref state")
                        })
                        .collect::<Vec<_>>()
                })
            });

            if tracing::level_filters::STATIC_MAX_LEVEL >= tracing::Level::TRACE {
                let span = tracing::Span::current();
                span.record("refs.len", refs.len() as i64);
            }
            refs
        });

        let current_span = tracing::Span::current().clone();
        let db = repository.get_message_db();

        let desired_timeout = { *timeout.lock() };
        let time_limits = ExecutionTimeLimits::production(desired_timeout, &node_config);
        let thread = std::thread::Builder::new()
            .name(format!("Produce block {}", &thread_id_clone))
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let _current_span_scope = current_span.enter();

                tracing::trace!("start production thread");
                let (
                    block,
                    result_state,
                    active_block_producer_threads,
                    cross_thread_ref_data,
                    external_messages_processed_count,
                    ext_msg_feedbacks,
                ) = producer
                    // TODO: add refs to other thread states in case of sync
                    .produce(
                        thread_id_clone,
                        initial_state_clone,
                        refs.iter(),
                        control_rx,
                        db.clone(),
                        &time_limits,
                    )
                    .expect("Failed to produce block");

                (
                    block,
                    result_state,
                    active_block_producer_threads,
                    cross_thread_ref_data,
                    // Offset processed external messages
                    initial_external_messages_progress.next(external_messages_processed_count),
                    ext_msg_feedbacks,
                )
            })
            .unwrap();
        let corrected_timeout = timeout_correction.get_production_timeout(desired_timeout);
        tracing::trace!("Sleep for {corrected_timeout:?}");

        trace_span!("sleep").in_scope(|| {
            sleep(corrected_timeout);
        });

        tracing::trace!("Send signal to stop production");
        let _ = control_tx.send(());

        let (
            mut block,
            result_state,
            new_active_block_producer_threads,
            cross_thread_ref_data,
            progress,
            ext_msg_feedbacks,
        ) = thread.join().expect("Failed to join producer thread");
        tracing::trace!("Produced block: {}", block);
        block_flow_trace_with_time(
            Some(start_time),
            "production",
            &block.identifier(),
            &producer_node_id,
            [],
        );
        if let Ok(()) = external_control_rx.try_recv() {
            return true;
        }
        // Update common section
        let mut common_section = block.get_common_section().clone();
        common_section.acks = aggregated_acks;
        common_section.nacks = aggregated_nacks.clone();
        block.set_common_section(common_section, false).expect("Failed to set common section");
        external_messages_queue
            .set_progress(&block.identifier(), progress)
            .expect("Failed to set progress");

        if tracing::level_filters::STATIC_MAX_LEVEL >= tracing::Level::TRACE {
            if let Ok(info) = block.tvm_block().info.read_struct() {
                use tvm_block::GetRepresentationHash;
                let span = tracing::Span::current();
                span.record("block.seq_no", info.seq_no() as i64);
                span.record("block.hash", info.hash().unwrap_or_default().as_hex_string());
            }
        }

        // DEBUGGING
        // let mut parent_state = repo_clone.get_optimistic_state(&block.parent())
        // .expect("Must not fail")
        // .expect("Must have state");
        // assert!(block.get_common_section().refs.is_empty());
        // tracing::trace!(
        // "debug_apply_state: block {} on {:?}",
        // block.identifier(),
        // parent_state.get_shard_state_as_cell().repr_hash(),
        // );
        // parent_state.apply_block(&block, [].iter()).unwrap();
        //

        *active_block_producer_threads = new_active_block_producer_threads;
        *initial_state = result_state;
        repository
            .store_optimistic_in_cache(initial_state.clone())
            .expect("Failed to store initial state");

        let span_save_cross_thread_refs =
            trace_span!("save cross thread refs", refs_len = cross_thread_ref_data.refs().len());
        shared_services
            .exec(|e| {
                span_save_cross_thread_refs.in_scope(|| {
                    e.cross_thread_ref_data_service.set_cross_thread_ref_data(cross_thread_ref_data)
                })
            })
            .expect("Failed to save cross-thread ref data");
        drop(span_save_cross_thread_refs);

        let block_id = block.identifier();
        trace_span!("save state").in_scope(|| {
            tracing::trace!("Save produced block");
            let mut blocks = produced_blocks.lock();
            let produced_data = (block, initial_state.clone(), ext_msg_feedbacks);
            blocks.push(produced_data);
        });
        tracing::trace!("End block production process iteration");
        let production_time = production_time.elapsed();

        metrics.as_ref().inspect(|m| {
            m.report_block_production_time_and_correction(
                production_time.as_millis(),
                timeout_correction.get_correction(),
                &thread_id_clone,
            )
        });

        timeout_correction.report_last_production(production_time);
        if production_time < desired_timeout {
            sleep(desired_timeout - production_time);
        }
        block_flow_trace("finish production", &block_id, &producer_node_id, []);
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_thread_production(
        &mut self,
        thread_id: &ThreadIdentifier,
        prev_block_id: &BlockIdentifier,
        received_acks: Arc<Mutex<Vec<Envelope<GoshBLS, AckData>>>>,
        received_nacks: Arc<Mutex<Vec<Envelope<GoshBLS, NackData>>>>,
        block_state_repository: BlockStateRepository,
        nack_set_cache: Arc<Mutex<FixedSizeHashSet<UInt256>>>,
        external_messages: ExternalMessagesThreadState,
    ) -> anyhow::Result<()> {
        tracing::trace!(
            "BlockProducerProcess start production for thread: {:?}, initial_block_id:{:?}",
            thread_id,
            prev_block_id,
        );
        if self.active_producer_thread.is_some() {
            tracing::error!(
                "Logic failure: node should not start production for the same thread several times"
            );
            return Ok(());
        }
        tracing::trace!("start_thread_production: loading state to start production");
        let mut initial_state = {
            if let Some(state) = match &self.optimistic_state_cache {
                Some(state) => {
                    tracing::trace!(
                        "start_thread_production: producer process contains state cache for this thread: {:?} {prev_block_id:?}",
                        state.block_id
                    );
                    if &state.block_id == prev_block_id {
                        tracing::trace!("start_thread_production: use cached state");
                        Some(state.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            } {
                state
            } else {
                tracing::trace!(
                    "start_thread_production: cached state block id is not appropriate. Load state from repo"
                );
                if let Some(state) = self.repository.get_optimistic_state(
                    prev_block_id,
                    thread_id,
                    Arc::clone(&nack_set_cache),
                    None,
                )? {
                    state
                } else if prev_block_id == &BlockIdentifier::default() {
                    self.repository.get_zero_state_for_thread(thread_id)?
                } else {
                    panic!("Failed to find optimistic in repository for block {:?}", &prev_block_id)
                }
            }
        };

        let blockchain_config = Arc::clone(&self.blockchain_config);

        let repo_clone = self.repository.clone();
        let produced_blocks = self.produced_blocks.clone();
        let timeout = self.block_produce_timeout.clone();
        let thread_id_clone = *thread_id;
        let (control_tx, external_control_rx) = instrumented_channel(
            self.metrics.clone(),
            crate::helper::metrics::PRODUCE_CONTROL_CHANNEL,
        );
        let (epoch_block_keeper_data_tx, epoch_block_keeper_data_rx) = instrumented_channel(
            self.metrics.clone(),
            crate::helper::metrics::EPOCH_BK_DATA_CHANNEL,
        );
        let mut shared_services = self.shared_services.clone();
        let producer_node_id = self.producer_node_id.clone();
        let thread_count_soft_limit = self.thread_count_soft_limit;
        let parallelization_level = self.parallelization_level;
        let block_keeper_epoch_code_hash = self.block_keeper_epoch_code_hash.clone();
        let metrics = self.repository.get_metrics();
        let accounts_repo = self.repository.accounts_repository().clone();
        let node_config = self.node_config.clone();
        let handler = std::thread::Builder::new()
            .name(format!("Production {}", &thread_id_clone))
            .spawn(move || {
                let mut active_block_producer_threads = vec![];
                // Note:
                // This loop runs infinitely till the interrupt signal generating new blocks
                //
                // Note:
                // Repository in this case can be assumed a shared state between all threads.
                // Using repository threads can find messages incoming from other threads.
                // It is also possible to track blocks dependencies through repository.
                // TODO: think if it is the best solution given all circumstances
                let mut timeout_correction = ProductionTimeoutCorrection::default();
                loop {
                    let stopped = Self::produce_next(
                        node_config.clone(),
                        &mut initial_state,
                        blockchain_config.clone(),
                        producer_node_id.clone(),
                        thread_count_soft_limit,
                        parallelization_level,
                        block_keeper_epoch_code_hash.clone(),
                        produced_blocks.clone(),
                        timeout.clone(),
                        &mut timeout_correction,
                        thread_id_clone,
                        &epoch_block_keeper_data_rx,
                        &mut shared_services,
                        &mut active_block_producer_threads,
                        received_acks.clone(),
                        received_nacks.clone(),
                        block_state_repository.clone(),
                        accounts_repo.clone(),
                        &external_control_rx,
                        metrics.clone(),
                        &external_messages,
                        &repo_clone,
                    );
                    if stopped {
                        trace_span!("store optimistic").in_scope(|| {
                            repo_clone
                                .store_optimistic(initial_state.clone())
                                .expect("Failed to store optimistic state");
                            initial_state.changed_accounts.clear();
                            tracing::trace!("Stop production process: {}", &thread_id_clone);
                        });
                        return initial_state;
                    }
                }
            })?;
        self.active_producer_thread = Some((handler, control_tx));
        self.epoch_block_keeper_data_senders = Some(epoch_block_keeper_data_tx);
        Ok(())
    }

    pub fn write_block_to_db(
        &mut self,
        block: Envelope<GoshBLS, AckiNackiBlock>,
        mut optimistic_state: OptimisticStateImpl,
    ) -> anyhow::Result<()> {
        self.archive_sender.send((block, Some(optimistic_state.get_shard_state()), None))?;
        Ok(())
    }

    pub fn stop_thread_production(&mut self, thread_id: &ThreadIdentifier) -> anyhow::Result<()> {
        tracing::trace!("stop_thread_production: {thread_id:?}");
        if let Some((thread, control)) = self.active_producer_thread.take() {
            tracing::trace!("Stop production for thread {thread_id:?}");
            control.send(())?;
            let last_optimistic_state = thread
                .join()
                .map_err(|_| anyhow::format_err!("Failed to get last state on production stop"))?;
            self.optimistic_state_cache = Some(last_optimistic_state);
            let _ = self.get_produced_blocks();
        }
        Ok(())
    }

    pub fn get_produced_blocks(
        &mut self,
    ) -> Vec<(AckiNackiBlock, OptimisticStateImpl, ExtMsgFeedbackList, Instant)> {
        if let Some(handler) = self.active_producer_thread.as_ref() {
            assert!(!handler.0.is_finished());
        }
        let mut blocks = self.produced_blocks.lock();
        let now = Instant::now();
        let mut blocks_vec: Vec<(
            AckiNackiBlock,
            OptimisticStateImpl,
            ExtMsgFeedbackList,
            Instant,
        )> = blocks
            .drain(..)
            .map(|(block, state, feedbacks)| (block, state, feedbacks, now))
            .collect();
        blocks_vec.sort_by(|a, b| a.0.seq_no().cmp(&b.0.seq_no()));
        blocks_vec
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        tracing::trace!("set timeout for production process: {timeout:?}");
        let mut block_produce_timeout = self.block_produce_timeout.lock();
        *block_produce_timeout = timeout;
    }

    pub fn send_epoch_message(&self, data: BlockKeeperData) {
        if let Some(sender) = self.epoch_block_keeper_data_senders.as_ref() {
            let res = sender.send(data);
            tracing::trace!("send epoch message to producer res: {res:?}");
        }
    }
}

fn aggregate_acks(
    mut received_acks: Vec<Envelope<GoshBLS, AckData>>,
) -> anyhow::Result<Vec<Envelope<GoshBLS, AckData>>> {
    let mut aggregated_acks = HashMap::new();
    tracing::trace!("Aggregate acks start len: {}", received_acks.len());
    for ack in &received_acks {
        let block_id = ack.data().block_id.clone();
        tracing::trace!("Aggregate acks block id: {:?}", block_id);
        aggregated_acks
            .entry(block_id)
            .and_modify(|aggregated_ack: &mut Envelope<GoshBLS, AckData>| {
                let mut merged_signatures_occurences = aggregated_ack.clone_signature_occurrences();
                let initial_signatures_count = merged_signatures_occurences.len();
                let incoming_signature_occurences = ack.clone_signature_occurrences();
                for signer_index in incoming_signature_occurences.keys() {
                    let new_count = (*merged_signatures_occurences.get(signer_index).unwrap_or(&0))
                        + (*incoming_signature_occurences.get(signer_index).unwrap());
                    merged_signatures_occurences.insert(*signer_index, new_count);
                }
                merged_signatures_occurences.retain(|_k, count| *count > 0);

                if merged_signatures_occurences.len() > initial_signatures_count {
                    let aggregated_signature = aggregated_ack.aggregated_signature();
                    let merged_aggregated_signature =
                        GoshBLS::merge(aggregated_signature, ack.aggregated_signature())
                            .expect("Failed to merge attestations");
                    *aggregated_ack = Envelope::<GoshBLS, AckData>::create(
                        merged_aggregated_signature,
                        merged_signatures_occurences,
                        aggregated_ack.data().clone(),
                    );
                }
            })
            .or_insert(ack.clone());
    }
    tracing::trace!("Aggregate acks result len: {:?}", aggregated_acks.len());
    received_acks.clear();
    Ok(aggregated_acks.values().cloned().collect())
}

// TODO: fix this function nacks can't be aggregated based on block id
fn aggregate_nacks(
    mut received_nacks: Vec<Envelope<GoshBLS, NackData>>,
) -> anyhow::Result<Vec<Envelope<GoshBLS, NackData>>> {
    let mut aggregated_nacks = HashMap::new();
    tracing::trace!("Aggregate nacks start len: {}", received_nacks.len());
    for nack in &received_nacks {
        let block_id = nack.data().block_id.clone();
        tracing::trace!("Aggregate nacks block id: {:?}", block_id);
        aggregated_nacks
            .entry(block_id)
            .and_modify(|aggregated_nack: &mut Envelope<GoshBLS, NackData>| {
                let mut merged_signatures_occurences =
                    aggregated_nack.clone_signature_occurrences();
                let initial_signatures_count = merged_signatures_occurences.len();
                let incoming_signature_occurences = nack.clone_signature_occurrences();
                for signer_index in incoming_signature_occurences.keys() {
                    let new_count = (*merged_signatures_occurences.get(signer_index).unwrap_or(&0))
                        + (*incoming_signature_occurences.get(signer_index).unwrap());
                    merged_signatures_occurences.insert(*signer_index, new_count);
                }
                merged_signatures_occurences.retain(|_k, count| *count > 0);

                if merged_signatures_occurences.len() > initial_signatures_count {
                    let aggregated_signature = aggregated_nack.aggregated_signature();
                    let merged_aggregated_signature =
                        GoshBLS::merge(aggregated_signature, nack.aggregated_signature())
                            .expect("Failed to merge attestations");
                    *aggregated_nack = Envelope::<GoshBLS, NackData>::create(
                        merged_aggregated_signature,
                        merged_signatures_occurences,
                        aggregated_nack.data().clone(),
                    );
                }
            })
            .or_insert(nack.clone());
    }
    tracing::trace!("Aggregate nacks result len: {:?}", aggregated_nacks.len());
    received_nacks.clear();
    Ok(aggregated_nacks.values().cloned().collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use std::time::Instant;

    use itertools::Itertools;
    use parking_lot::Mutex;
    use testdir::testdir;

    use crate::block::producer::process::TVMBlockProducerProcess;
    use crate::config::load_blockchain_config;
    use crate::external_messages::ExternalMessagesThreadState;
    use crate::message_storage::MessageDurableStorage;
    use crate::multithreading::routing::service::RoutingService;
    use crate::node::associated_types::NodeIdentifier;
    use crate::node::shared_services::SharedServices;
    use crate::repository::repository_impl::RepositoryImpl;
    use crate::tests::project_root;
    use crate::types::BlockIdentifier;
    use crate::types::ThreadIdentifier;
    use crate::utilities::FixedSizeHashSet;

    #[test]
    #[ignore]
    fn test_producer() -> anyhow::Result<()> {
        let root_dir = testdir!();
        crate::tests::init_db(&root_dir).expect("Failed to init DB 1");
        let mut config = crate::tests::default_config(NodeIdentifier::test(1));
        let config_dir = project_root().join("node/tests/resources/config");
        config.local.blockchain_config_path = config_dir.join("blockchain.conf.json");
        config.local.key_path =
            config_dir.join("block_keeper1_bls.keys.json").to_string_lossy().to_string();
        config.local.zerostate_path = config_dir.join("zerostate");
        config.local.block_keeper_seed_path =
            config_dir.join("block_keeper1_bls.keys.json").to_string_lossy().to_string();

        let block_state_repository =
            crate::node::block_state::repository::BlockStateRepository::new(
                root_dir.join("block-state"),
            );
        let message_db = MessageDurableStorage::new(PathBuf::from("./tmp/message_storage5"))?;
        let repository = RepositoryImpl::new(
            root_dir.clone(),
            Some(config.local.zerostate_path.clone()),
            1,
            1,
            SharedServices::start(
                RoutingService::stub().0,
                root_dir.clone(),
                None,
                config.global.thread_load_threshold,
                config.global.thread_load_window_size,
            ),
            Arc::new(Mutex::new(FixedSizeHashSet::new(10))),
            true,
            block_state_repository.clone(),
            None,
            message_db.clone(),
        );
        let (router, _router_rx) = RoutingService::stub();
        let (sender, _) = std::sync::mpsc::channel();
        let mut production_process = TVMBlockProducerProcess::builder()
            .metrics(repository.get_metrics())
            .node_config(config.clone())
            .repository(repository.clone())
            .block_keeper_epoch_code_hash(config.global.block_keeper_epoch_code_hash.clone())
            .producer_node_id(config.local.node_id.clone())
            .blockchain_config(Arc::new(load_blockchain_config(
                &config.local.blockchain_config_path,
            )?))
            .parallelization_level(config.local.parallelization_level)
            .archive_sender(sender)
            .shared_services(SharedServices::start(
                router,
                root_dir.clone(),
                None,
                config.global.thread_load_threshold,
                config.global.thread_load_window_size,
            ))
            .block_produce_timeout(Arc::new(Mutex::new(Duration::from_millis(
                config.global.time_to_produce_block_millis,
            ))))
            .thread_count_soft_limit(config.global.thread_count_soft_limit)
            .build();

        let thread_id = ThreadIdentifier::default();

        production_process.start_thread_production(
            &thread_id,
            &BlockIdentifier::default(),
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
            block_state_repository.clone(),
            Arc::new(Mutex::new(FixedSizeHashSet::new(10))),
            ExternalMessagesThreadState::builder()
                .with_external_messages_file_path(root_dir.join("ext-messages.data"))
                .with_block_progress_data_dir(root_dir.join("external-messages-progress"))
                .with_report_metrics(None)
                .with_thread_id(thread_id)
                .build()?,
        )?;

        let running_time = Instant::now();
        let mut since_last_block = running_time;
        let mut sum = 0;
        let mut count = 0;
        while running_time.elapsed() < Duration::from_secs(10) {
            sleep(Duration::from_millis(1));
            let blocks = production_process.get_produced_blocks();
            if !blocks.is_empty() {
                count += 1;
                sum += since_last_block.elapsed().as_millis();
                println!(
                    "block produced: {} tx, {} ms",
                    blocks.iter().map(|(block, ..)| format!("{}", block.tx_cnt())).join(","),
                    since_last_block.elapsed().as_millis(),
                );
                since_last_block = Instant::now();
            }
        }

        let avg_time = sum / count;
        println!("Average time: {} ms", avg_time);
        production_process.stop_thread_production(&thread_id)?;
        assert!(avg_time > 320 && avg_time < 340);
        Ok(())
    }
}
