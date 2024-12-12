// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use database::documents_db::SerializedItem;
use tvm_block::ShardStateUnsplit;
use tvm_types::AccountId;

use super::repository_impl::RepositoryImpl;
use crate::block_keeper_system::BlockKeeperSet;
use crate::bls::envelope::Envelope;
use crate::bls::GoshBLS;
#[cfg(test)]
use crate::message::message_stub::MessageStub;
use crate::node::associated_types::AttestationData;
use crate::node::NodeIdentifier;
use crate::repository::optimistic_state::OptimisticState;
use crate::repository::Repository;
use crate::types::AccountAddress;
use crate::types::AckiNackiBlock;
use crate::types::BlockEndLT;
use crate::types::BlockIdentifier;
use crate::types::BlockInfo;
use crate::types::BlockSeqNo;
use crate::types::DAppIdentifier;
use crate::types::ThreadIdentifier;
use crate::types::ThreadsTable;

#[cfg(test)]
#[derive(Clone)]
pub struct OptimisticStateStub {
    _key: u64,
    _acc: u16,
    _state: tvm_block::Account,
}

#[cfg(test)]
impl OptimisticState for OptimisticStateStub {
    type Cell = ();
    type Message = MessageStub;
    type ShardState = ShardStateUnsplit;

    fn get_remaining_ext_messages(
        &self,
        _repository: &RepositoryImpl,
    ) -> anyhow::Result<Vec<Self::Message>> {
        todo!()
    }

    fn get_block_seq_no(&self) -> &BlockSeqNo {
        todo!()
    }

    fn get_block_id(&self) -> &BlockIdentifier {
        todo!()
    }

    fn serialize_into_buf(&mut self) -> anyhow::Result<Vec<u8>> {
        todo!()
    }

    fn get_shard_state(&mut self) -> Self::ShardState {
        todo!()
    }

    fn get_block_info(&self) -> &BlockInfo {
        todo!()
    }

    fn get_shard_state_as_cell(&mut self) -> Self::Cell {
        todo!()
    }

    fn apply_block(&mut self, _block_candidate: &AckiNackiBlock<GoshBLS>) -> anyhow::Result<()> {
        todo!()
    }

    fn get_thread_id(&self) -> &ThreadIdentifier {
        todo!()
    }

    fn get_threads_table(&self) -> &ThreadsTable {
        todo!()
    }

    fn split_state_for_mask(
        self,
        _threads_table: ThreadsTable,
        _thread_id: ThreadIdentifier,
    ) -> anyhow::Result<(Self, Self)> {
        todo!()
    }

    fn does_account_belong_to_the_state(&self, _account_id: &AccountId) -> anyhow::Result<bool> {
        todo!()
    }

    fn get_dapp_id_table(&self) -> &HashMap<AccountAddress, (Option<DAppIdentifier>, BlockEndLT)> {
        todo!()
    }

    fn merge_threads_table(&mut self, _another_state: &Self) -> anyhow::Result<()> {
        todo!()
    }

    fn merge_dapp_id_tables(&mut self, _another_state: &Self) -> anyhow::Result<()> {
        todo!()
    }

    fn add_unprocessed_messages(&mut self, _messages: Vec<Self::Message>) {
        todo!()
    }

    fn get_messages_for_another_thread(
        &self,
        _another_state: &Self,
    ) -> anyhow::Result<Vec<Self::Message>> {
        todo!()
    }

    fn does_state_has_messages_to_other_threads(&self) -> anyhow::Result<bool> {
        todo!()
    }
}

#[cfg(test)]
pub struct RepositoryStub {
    _storage: HashMap<BlockIdentifier, Envelope<GoshBLS, AckiNackiBlock<GoshBLS>>>,
    optimistic_state: HashMap<BlockIdentifier, <Self as Repository>::OptimisticState>,
    finalized_states: HashMap<ThreadIdentifier, OptimisticStateStub>,
}

#[cfg(test)]
impl Default for RepositoryStub {
    fn default() -> Self {
        Self::new()
    }
}

impl RepositoryStub {
    pub fn new() -> Self {
        Self {
            _storage: HashMap::new(),
            optimistic_state: HashMap::new(),
            finalized_states: HashMap::new(),
        }
    }
}

#[cfg(test)]
impl From<Vec<u8>> for OptimisticStateStub {
    fn from(_value: Vec<u8>) -> Self {
        todo!()
    }
}

#[cfg(test)]
impl From<OptimisticStateStub> for Vec<u8> {
    fn from(_val: OptimisticStateStub) -> Self {
        todo!()
    }
}

#[cfg(test)]
impl Repository for RepositoryStub {
    type Attestation = Envelope<GoshBLS, AttestationData>;
    type BLS = GoshBLS;
    type CandidateBlock = Envelope<GoshBLS, AckiNackiBlock<GoshBLS>>;
    type EnvelopeSignerIndex = u16;
    type NodeIdentifier = NodeIdentifier;
    type OptimisticState = OptimisticStateStub;
    type StateSnapshot = OptimisticStateStub;

    fn dump_sent_attestations(
        &self,
        _data: HashMap<ThreadIdentifier, Vec<(BlockSeqNo, Self::Attestation)>>,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn load_sent_attestations(
        &self,
    ) -> anyhow::Result<HashMap<ThreadIdentifier, Vec<(BlockSeqNo, Self::Attestation)>>> {
        todo!()
    }

    fn get_block(
        &self,
        _identifier: &BlockIdentifier,
    ) -> anyhow::Result<Option<Self::CandidateBlock>> {
        todo!();
    }

    fn get_block_from_repo_or_archive(
        &self,
        _block_id: &BlockIdentifier,
    ) -> anyhow::Result<<Self as Repository>::CandidateBlock> {
        todo!()
    }

    fn get_block_from_repo_or_archive_by_seq_no(
        &self,
        _block_seq_no: &BlockSeqNo,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<Vec<<Self as Repository>::CandidateBlock>> {
        todo!()
    }

    fn list_blocks_with_seq_no(
        &self,
        _seq_no: &BlockSeqNo,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<Vec<Envelope<GoshBLS, AckiNackiBlock<GoshBLS>>>> {
        Ok(vec![])
    }

    fn select_thread_last_finalized_block(
        &self,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<(BlockIdentifier, BlockSeqNo)> {
        todo!();
    }

    fn select_thread_last_main_candidate_block(
        &self,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<(BlockIdentifier, BlockSeqNo)> {
        todo!();
    }

    fn mark_block_as_accepted_as_main_candidate(
        &self,
        _block_id: &BlockIdentifier,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_block_accepted_as_main_candidate(
        &self,
        _block_id: &BlockIdentifier,
    ) -> anyhow::Result<Option<bool>> {
        Ok(None)
    }

    fn mark_block_as_finalized(&mut self, _block: &Self::CandidateBlock) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_block_finalized(&self, _block_id: &BlockIdentifier) -> anyhow::Result<Option<bool>> {
        Ok(None)
    }

    fn get_optimistic_state(
        &self,
        block_id: &BlockIdentifier,
    ) -> anyhow::Result<Option<OptimisticStateStub>> {
        Ok(self.optimistic_state.get(block_id).map(|s| s.to_owned()))
    }

    fn is_optimistic_state_present(&self, _block_id: &BlockIdentifier) -> bool {
        todo!()
    }

    fn store_block<T: Into<Self::CandidateBlock>>(&self, _block: T) -> anyhow::Result<()> {
        Ok(())
    }

    fn erase_block_and_optimistic_state(
        &self,
        _block_id: &BlockIdentifier,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<()> {
        todo!();
    }

    fn erase_block(
        &self,
        _block_id: &BlockIdentifier,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<()> {
        todo!()
    }

    // fn set_optimistic_as_is(&self, _optimistic: Self::OptimisticState) ->
    // anyhow::Result<()> { todo!()
    // }

    fn list_stored_thread_finalized_blocks(
        &self,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<Vec<(BlockIdentifier, BlockSeqNo)>> {
        todo!();
    }

    fn delete_external_messages(&self, _count: usize) -> anyhow::Result<()> {
        todo!()
    }

    fn add_external_message<T>(&mut self, _messages: Vec<T>) -> anyhow::Result<()>
    where
        T: Into<<Self::OptimisticState as OptimisticState>::Message>,
    {
        todo!()
    }

    fn mark_block_as_verified(&self, _block_id: &BlockIdentifier) -> anyhow::Result<()> {
        todo!()
    }

    fn is_block_verified(&self, _block_id: &BlockIdentifier) -> anyhow::Result<bool> {
        todo!()
    }

    fn take_state_snapshot(
        &self,
        _block_id: &BlockIdentifier,
        _block_producer_groups: HashMap<ThreadIdentifier, Vec<Self::NodeIdentifier>>,
        _block_keeper_set: HashMap<ThreadIdentifier, BTreeMap<BlockSeqNo, BlockKeeperSet>>,
    ) -> anyhow::Result<Self::StateSnapshot> {
        todo!()
    }

    fn convert_state_data_to_snapshot(
        &self,
        _serialized_state: Vec<u8>,
        _block_producer_groups: HashMap<ThreadIdentifier, Vec<Self::NodeIdentifier>>,
        _block_keeper_set: HashMap<ThreadIdentifier, BTreeMap<BlockSeqNo, BlockKeeperSet>>,
    ) -> anyhow::Result<Self::StateSnapshot> {
        todo!()
    }

    fn set_state_from_snapshot(
        &mut self,
        _block_id: &BlockIdentifier,
        _snapshot: Self::StateSnapshot,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<(
        HashMap<ThreadIdentifier, Vec<Self::NodeIdentifier>>,
        HashMap<ThreadIdentifier, BTreeMap<BlockSeqNo, BlockKeeperSet>>,
    )> {
        todo!()
    }

    fn sync_accounts_from_state(
        &mut self,
        _shard_state: Arc<ShardStateUnsplit>,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn save_account_diffs(
        &self,
        _block_id: BlockIdentifier,
        _accounts: HashMap<String, SerializedItem>,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn last_stored_block_by_seq_no(
        &self,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<BlockSeqNo> {
        todo!()
    }

    fn store_optimistic<T: Into<Self::OptimisticState>>(
        &mut self,
        _state: T,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn get_block_id_by_seq_no(
        &self,
        _block_seq_no: &BlockSeqNo,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<Vec<BlockIdentifier>> {
        todo!()
    }

    fn get_latest_block_id_with_producer_group_change(
        &self,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<BlockIdentifier> {
        todo!()
    }

    fn clear_ext_messages_queue_by_time(&self) -> anyhow::Result<()> {
        todo!()
    }

    fn clear_verification_markers(
        &self,
        _starting_block_id: &BlockSeqNo,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn is_candidate_block_can_be_applied(&self, _block: &Self::CandidateBlock) -> bool {
        todo!()
    }

    fn get_zero_state_for_thread(
        &self,
        _thread_id: &ThreadIdentifier,
    ) -> anyhow::Result<Self::OptimisticState> {
        todo!()
    }

    fn list_finalized_states(
        &self,
    ) -> impl Iterator<Item = (&'_ ThreadIdentifier, &'_ Self::OptimisticState)> {
        self.finalized_states.iter()
    }
}
