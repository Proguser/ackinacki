// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::block::keeper::process::BlockKeeperProcess;
use crate::block::producer::process::BlockProducerProcess;
use crate::block::producer::BlockProducer;
use crate::block_keeper_system::BlockKeeperSet;
use crate::block_keeper_system::BlockKeeperSetChange;
use crate::block_keeper_system::BlockKeeperStatus;
use crate::bls::envelope::Envelope;
use crate::bls::gosh_bls::PubKey;
use crate::bls::BLSSignatureScheme;
use crate::node::associated_types::AttestationData;
use crate::node::associated_types::OptimisticStateFor;
use crate::node::attestation_processor::AttestationProcessor;
use crate::node::services::sync::StateSyncService;
use crate::node::Node;
use crate::node::NodeIdentifier;
use crate::node::SignerIndex;
use crate::repository::optimistic_state::OptimisticState;
use crate::repository::Repository;
use crate::types::AckiNackiBlock;
use crate::types::BlockSeqNo;
use crate::types::ThreadIdentifier;

const EPOCH_TOUCH_RETRY_TIME_DELTA: u32 = 5;

impl<TBLSSignatureScheme, TStateSyncService, TBlockProducerProcess, TValidationProcess, TRepository, TAttestationProcessor, TRandomGenerator>
Node<TBLSSignatureScheme, TStateSyncService, TBlockProducerProcess, TValidationProcess, TRepository, TAttestationProcessor, TRandomGenerator>
    where
        TBLSSignatureScheme: BLSSignatureScheme<PubKey = PubKey> + Clone,
        <TBLSSignatureScheme as BLSSignatureScheme>::PubKey: PartialEq,
        TBlockProducerProcess:
        BlockProducerProcess< Repository = TRepository>,
        TValidationProcess: BlockKeeperProcess<
            BLSSignatureScheme = TBLSSignatureScheme,
            CandidateBlock = Envelope<TBLSSignatureScheme, AckiNackiBlock<TBLSSignatureScheme>>,

            OptimisticState = OptimisticStateFor<TBlockProducerProcess>,
        >,
        TBlockProducerProcess: BlockProducerProcess<
            BLSSignatureScheme = TBLSSignatureScheme,
            CandidateBlock = Envelope<TBLSSignatureScheme, AckiNackiBlock<TBLSSignatureScheme>>,

        >,
        TRepository: Repository<
            BLS = TBLSSignatureScheme,
            EnvelopeSignerIndex = SignerIndex,

            CandidateBlock = Envelope<TBLSSignatureScheme, AckiNackiBlock<TBLSSignatureScheme>>,
            OptimisticState = OptimisticStateFor<TBlockProducerProcess>,
            NodeIdentifier = NodeIdentifier,
            Attestation = Envelope<TBLSSignatureScheme, AttestationData>,
        >,
        <<TBlockProducerProcess as BlockProducerProcess>::BlockProducer as BlockProducer>::Message: Into<
            <<TBlockProducerProcess as BlockProducerProcess>::OptimisticState as OptimisticState>::Message,
        >,
        TStateSyncService: StateSyncService<
            Repository = TRepository
        >,
        TAttestationProcessor: AttestationProcessor<
            BlockAttestation = Envelope<TBLSSignatureScheme, AttestationData>,
            CandidateBlock = Envelope<TBLSSignatureScheme, AckiNackiBlock<TBLSSignatureScheme>>,
        >,
        TRandomGenerator: rand::Rng,
{
    // BP node checks current epoch contracts and sends touch message to finish them
    pub(crate) fn check_and_touch_block_keeper_epochs(
        &mut self,
        thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp() as u32;
        tracing::trace!("check block keepers: now={now}");
        let mut locked = self.block_keeper_sets.lock();
        let block_keeper_set = locked.get_mut(thread_id).expect("Failed to get block keeper set for thread");
        let mut current_bk_set = block_keeper_set.last_entry().expect("Block keeper set map should not be empty");
        for data in current_bk_set.get_mut().values_mut() {
            if data.epoch_finish_timestamp < now {
                tracing::trace!("Epoch is outdated: now={now} {data:?}");

                match data.status {
                    // If block keeper was not touched, send touch message, change its status
                    // and increase saved timestamp with 5 seconds
                    BlockKeeperStatus::Active => {
                        self.production_process.send_epoch_message(
                            &self.thread_id,
                            data.clone(),
                        );
                        data.epoch_finish_timestamp += EPOCH_TOUCH_RETRY_TIME_DELTA;
                        data.status = BlockKeeperStatus::CalledToFinish;
                    },
                    // If block keeper was already touched, touch it one more time and
                    // change status for not to change.
                    BlockKeeperStatus::CalledToFinish => {
                        self.production_process.send_epoch_message(
                            &self.thread_id,
                            data.clone(),
                        );
                        data.status = BlockKeeperStatus::Expired;
                    },
                    BlockKeeperStatus::Expired => {},
                }
            }
        }
        Ok(())
    }

    pub(crate) fn update_block_keeper_set_from_common_section(
        &mut self,
        block: &AckiNackiBlock<TBLSSignatureScheme>,
        thread_id: &ThreadIdentifier
    ) -> anyhow::Result<()> {
        let common_section = block.get_common_section();
        let mut block_keeper_sets = self.block_keeper_sets.lock().get(thread_id).expect("Failed to get block keeper sets for thread").clone();
        if !common_section.block_keeper_set_changes.is_empty() {
            tracing::trace!("update_block_keeper_set_from_common_section {block_keeper_sets:?} {:?}", common_section.block_keeper_set_changes);
            let block_seq_no = block.seq_no();
            let mut new_bk_set = block_keeper_sets.last_key_value().expect("Block keeper sets should not be empty").1.clone();
            // Process removes first, because remove and add can happen in one block
            for block_keeper_change in &common_section.block_keeper_set_changes {
                if let BlockKeeperSetChange::BlockKeeperRemoved((signer_index, block_keeper_data)) = block_keeper_change {
                    tracing::trace!("Remove block keeper key: {signer_index} {block_keeper_data:?}");
                    tracing::trace!("Remove block keeper key: {:?}", new_bk_set);
                    let block_keeper_data = new_bk_set.remove(signer_index);
                    tracing::trace!("Removed block keeper key: {:?}", block_keeper_data);
                }
            }
            for block_keeper_change in &common_section.block_keeper_set_changes {
                if let BlockKeeperSetChange::BlockKeeperAdded((signer_index, block_keeper_data)) = block_keeper_change {
                    tracing::trace!("insert block keeper key: {signer_index} {block_keeper_data:?}");
                    tracing::trace!("insert block keeper key: {:?}", new_bk_set);
                    new_bk_set.insert(*signer_index, block_keeper_data.clone());
                }
            }
            block_keeper_sets.insert(block_seq_no, new_bk_set);
            {
                self.block_keeper_sets.lock().insert(*thread_id, block_keeper_sets);
            }
            tracing::trace!("update_block_keeper_set_from_common_section finished {:?}", self.block_keeper_sets);
        }
        Ok(())
    }

    pub fn get_block_keeper_set(
        &self,
        block_seq_no: &BlockSeqNo,
        thread_id: &ThreadIdentifier,
    ) -> BlockKeeperSet {
        let block_keeper_sets_for_thread = self.block_keeper_sets.lock().get(thread_id).expect("Failed to get bk set for thread").clone();
        for (seq_no, bk_set) in block_keeper_sets_for_thread.iter().rev() {
            if seq_no > block_seq_no {
                continue;
            }
            return bk_set.clone();
        }
        panic!("Failed to find BK set for block with seq_no: {block_seq_no:?}")
    }

    pub fn current_block_keeper_set(
        &self,
        thread_id: &ThreadIdentifier,
    ) -> BlockKeeperSet {
        tracing::trace!("current_block_keeper_pubkeys for thread_id: {thread_id:?}, sets {:?}", self.block_keeper_sets);
        let block_keeper_sets_for_thread = self.block_keeper_sets.lock().get(thread_id).expect("Failed to get bk set for thread").clone();
        block_keeper_sets_for_thread.last_key_value().expect("Node should have latest BK set").1.clone()
    }

    pub fn get_block_keeper_pubkeys(
        &self,
        block_seq_no: &BlockSeqNo,
        thread_id: &ThreadIdentifier,
    ) -> HashMap<SignerIndex, PubKey> {
        self.get_block_keeper_set(block_seq_no, thread_id).into_iter().map(|(k, v)| (k, v.pubkey)).collect()
    }

    pub fn set_block_keeper_sets(
        &self,
        block_keeper_sets: HashMap<ThreadIdentifier, BTreeMap<BlockSeqNo, BlockKeeperSet>>
    ) {
        let mut bk_sets_ref = self.block_keeper_sets.lock();
        *bk_sets_ref = block_keeper_sets;
    }

    pub fn get_block_keeper_sets_for_all_threads(
        &self,
    ) -> HashMap<ThreadIdentifier, BTreeMap<BlockSeqNo, BlockKeeperSet>> {
        self.block_keeper_sets.lock().clone()
    }
}
