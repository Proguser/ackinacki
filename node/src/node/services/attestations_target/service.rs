// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::vec::Vec;

use typed_builder::TypedBuilder;

use crate::node::block_state::repository::BlockState;
use crate::node::block_state::state::AttestationsTarget;
use crate::node::block_state::unfinalized_ancestor_blocks::UnfinalizedAncestorBlocks;
use crate::node::block_state::unfinalized_ancestor_blocks::UnfinalizedAncestorBlocksSelectError;
use crate::node::unprocessed_blocks_collection::UnfinalizedBlocksSnapshot;
use crate::node::BlockStateRepository;
use crate::node::SignerIndex;
use crate::repository::repository_impl::RepositoryImpl;
use crate::repository::Repository;
use crate::types::BlockIdentifier;
use crate::types::BlockSeqNo;
use crate::types::ForkResolution;
use crate::types::ThreadIdentifier;
use crate::utilities::guarded::Guarded;
use crate::utilities::guarded::GuardedMut;

#[derive(TypedBuilder, Clone)]
pub struct AttestationsTargetService {
    repository: RepositoryImpl,
    block_state_repository: BlockStateRepository,
}

pub enum AttestationsSuccess {
    InitialAttestationsTargetMet,
    SecondaryAttestationsTargetMet,
}

pub enum AttestationsFailure {
    ChainIsTooShort,
    InitialAttestationsTargetIsNotMetResolvesFork,
    NotAllInitialAttestationTargetsSet,
    ThreadIdentifierIsNotSet,
    AttestationsAreNotVerifiedYet,
    ForkResolutionsAreNotSet,
    FailedToSaveBlockState,
    #[allow(non_camel_case_types)]
    InvalidBlock_TailDoesNotMeetCriteria,
}

trait AsChain {
    fn next(&mut self) -> Option<BlockState>;
    fn peek(&mut self, nth_child: usize) -> Option<impl TargetBlock>;
}
impl AsChain for VecDeque<BlockState> {
    fn next(&mut self) -> Option<BlockState> {
        self.pop_front()
    }

    fn peek(&mut self, nth_child: usize) -> Option<impl TargetBlock> {
        self.get(nth_child).cloned()
    }
}

impl AsChain for (VecDeque<BlockState>, Target) {
    fn next(&mut self) -> Option<BlockState> {
        self.0.pop_front()
    }

    fn peek(&mut self, nth_child: usize) -> Option<impl TargetBlock> {
        if nth_child == self.0.len() {
            Some(self.1.clone())
        } else {
            self.0.get(nth_child).map(|e| Target::Candidate(e.clone()))
        }
    }
}

trait TargetBlock {
    fn thread_identifier(&self) -> Option<ThreadIdentifier>;
    fn fork_resolutions(&self) -> Option<Vec<ForkResolution>>;
    fn attestations_for(&self, block_id: &BlockIdentifier) -> Option<HashSet<SignerIndex>>;
    fn has_attestations_target_met(&self) -> bool;
}

#[derive(Clone)]
enum Target {
    // Note: intentionally made tuple. It is easier to pass later in the code.
    Phantom(
        (ThreadIdentifier, HashMap<BlockIdentifier, HashSet<SignerIndex>>, Vec<ForkResolution>),
    ),
    Candidate(BlockState),
}

impl TargetBlock for BlockState {
    fn thread_identifier(&self) -> Option<ThreadIdentifier> {
        self.guarded(|e| *e.thread_identifier())
    }

    fn fork_resolutions(&self) -> Option<Vec<ForkResolution>> {
        self.guarded(|e| e.resolves_forks().clone())
    }

    fn attestations_for(&self, block_id: &BlockIdentifier) -> Option<HashSet<SignerIndex>> {
        self.guarded(|e| e.verified_attestations_for(block_id))
    }

    fn has_attestations_target_met(&self) -> bool {
        self.guarded(|e| e.has_attestations_target_met())
    }
}

impl TargetBlock
    for &(ThreadIdentifier, HashMap<BlockIdentifier, HashSet<SignerIndex>>, Vec<ForkResolution>)
{
    fn thread_identifier(&self) -> Option<ThreadIdentifier> {
        Some(self.0)
    }

    fn fork_resolutions(&self) -> Option<Vec<ForkResolution>> {
        Some(self.2.clone())
    }

    fn attestations_for(&self, block_id: &BlockIdentifier) -> Option<HashSet<SignerIndex>> {
        Some(self.1.get(block_id).cloned().unwrap_or_default())
    }

    fn has_attestations_target_met(&self) -> bool {
        false
    }
}

impl TargetBlock for Target {
    fn thread_identifier(&self) -> Option<ThreadIdentifier> {
        match self {
            Target::Candidate(e) => e.thread_identifier(),
            Target::Phantom(e) => e.thread_identifier(),
        }
    }

    fn fork_resolutions(&self) -> Option<Vec<ForkResolution>> {
        match self {
            Target::Candidate(e) => e.fork_resolutions(),
            Target::Phantom(e) => e.fork_resolutions(),
        }
    }

    fn attestations_for(&self, block_id: &BlockIdentifier) -> Option<HashSet<SignerIndex>> {
        match self {
            Target::Candidate(e) => e.attestations_for(block_id),
            Target::Phantom(e) => e.attestations_for(block_id),
        }
    }

    fn has_attestations_target_met(&self) -> bool {
        match self {
            Target::Candidate(e) => e.has_attestations_target_met(),
            Target::Phantom(e) => e.has_attestations_target_met(),
        }
    }
}

impl AttestationsTargetService {
    #[allow(clippy::mutable_key_type)]
    pub fn evaluate(&mut self, blocks: &UnfinalizedBlocksSnapshot) {
        for (block_state, _) in blocks.values() {
            let Some(thread_id) = block_state.guarded(|e| *e.thread_identifier()) else {
                continue;
            };
            let Ok((_, thread_last_finalized_block_seq_no)) =
                self.repository.select_thread_last_finalized_block(&thread_id)
            else {
                continue;
            };
            let _ = self.evaluate_chain(block_state, thread_last_finalized_block_seq_no);
        }
    }

    // TODO: expand errors set. Return actual errors instead of Ok(false)
    pub fn evaluate_if_next_block_ancestors_required_attestations_will_be_met(
        &self,
        thread_identifier: ThreadIdentifier,
        parent_block_identifier: BlockIdentifier,
        next_block_attestations: HashMap<BlockIdentifier, HashSet<SignerIndex>>,
        fork_resolutions: Vec<ForkResolution>,
    ) -> anyhow::Result<bool, UnfinalizedAncestorBlocksSelectError> {
        tracing::trace!("evaluate_if_next_block_ancestors_required_attestations_will_be_met: parent_block_identifier: {parent_block_identifier:?}, next_block_attestations: {next_block_attestations:?}");
        let Ok(tail) = self.block_state_repository.get(&parent_block_identifier) else {
            return Ok(false);
        };
        let Ok((_, thread_last_finalized_block_seq_no)) =
            self.repository.select_thread_last_finalized_block(&thread_identifier)
        else {
            return Ok(false);
        };
        let cutoff = thread_last_finalized_block_seq_no;
        let chain =
            self.block_state_repository.select_unfinalized_ancestor_blocks(&tail, cutoff)?;
        use AttestationsFailure::*;
        match self.evaluate_attestations(
            (
                VecDeque::<BlockState>::from(chain),
                Target::Phantom((thread_identifier, next_block_attestations, fork_resolutions)),
            ),
            |_| Ok(()),
            |_| Ok(()),
        ) {
            Ok(()) | Err(ChainIsTooShort) | Err(InitialAttestationsTargetIsNotMetResolvesFork) => {
                Ok(true)
            }

            Err(NotAllInitialAttestationTargetsSet)
            | Err(ThreadIdentifierIsNotSet)
            | Err(ForkResolutionsAreNotSet)
            | Err(InvalidBlock_TailDoesNotMeetCriteria)
            | Err(FailedToSaveBlockState)
            | Err(AttestationsAreNotVerifiedYet) => Ok(false),
        }
    }

    // Evaluate blocks.
    // Cutoff is set to the last finalized block in the **thread**
    // In case of a chain goes earlier than the cut off we can consider this chain
    // to be invalid.
    fn evaluate_chain(&mut self, tail: &BlockState, cutoff: BlockSeqNo) -> anyhow::Result<()> {
        use UnfinalizedAncestorBlocksSelectError::*;
        match self.block_state_repository.select_unfinalized_ancestor_blocks(tail, cutoff) {
            Ok(chain) => {
                use AttestationsFailure::*;
                match self.evaluate_attestations(
                    VecDeque::<BlockState>::from(chain),
                    |block| {
                        block
                            .guarded_mut(|e| -> anyhow::Result<()> {
                                e.set_has_initial_attestations_target_met()
                            })
                            .map_err(|_e| FailedToSaveBlockState)?;
                        Ok(())
                    },
                    |block| {
                        block
                            .guarded_mut(|e| -> anyhow::Result<()> {
                                e.set_has_attestations_target_met_in_a_resolved_fork_case()
                            })
                            .map_err(|_e| FailedToSaveBlockState)?;
                        Ok(())
                    },
                ) {
                    Ok(()) => Ok(()),
                    Err(ChainIsTooShort) => Ok(()),
                    Err(NotAllInitialAttestationTargetsSet) => Ok(()),
                    Err(ForkResolutionsAreNotSet) => Ok(()),
                    Err(ThreadIdentifierIsNotSet) => Ok(()),

                    Err(InvalidBlock_TailDoesNotMeetCriteria) => {
                        tail.guarded_mut(|e| e.set_invalidated())
                    }
                    Err(FailedToSaveBlockState) => Ok(()),
                    Err(AttestationsAreNotVerifiedYet) => Ok(()),
                    Err(InitialAttestationsTargetIsNotMetResolvesFork) => Ok(()),
                }
            }
            Err(IncompleteHistory) => Ok(()),

            // The block is earlier than the cutoff for the thread.
            //
            // We didn't hit any finalized block
            // AND this chain is referencing something earlier than the last finalized.
            // This means that this chain CAN NOT be finalized (it means invalid).
            Err(BlockSeqNoCutoff(chain)) => {
                tracing::error!("evaluate_chain: BlockSeqNoCutoff: {chain:?}");
                self.invalidate_blocks(chain)
            }
            Err(InvalidatedParent(chain)) => {
                tracing::error!("evaluate_chain: InvalidatedParent: {chain:?}");
                self.invalidate_blocks(chain)
            }

            // No worries we will try again in a few.
            // May be shoud touch repository though
            Err(FailedToLoadBlockState) => Ok(()),
        }
    }

    fn invalidate_blocks(&mut self, blocks: Vec<BlockState>) -> anyhow::Result<()> {
        for block in blocks {
            tracing::trace!("invalidate blocks {:?}", block.lock().block_identifier());
            block.lock().set_invalidated()?;
        }
        Ok(())
    }

    fn evaluate_attestations<FPrimary, FSecordary>(
        &self,
        mut chain: impl AsChain,
        mut on_initial_attestations_target_met: FPrimary,
        mut on_secondary_attestations_target_met: FSecordary,
    ) -> std::result::Result<(), AttestationsFailure>
    where
        FPrimary: FnMut(BlockState) -> anyhow::Result<(), AttestationsFailure>,
        FSecordary: FnMut(BlockState) -> anyhow::Result<(), AttestationsFailure>,
    {
        loop {
            let Some(block) = chain.next() else {
                return Ok(());
            };
            if block.has_attestations_target_met() {
                continue;
            }
            let (initial_attestations_target, thread_identifier) =
                block.guarded(|e| (*e.initial_attestations_target(), *e.thread_identifier()));
            let Some(thread_identifier) = thread_identifier else {
                return Err(AttestationsFailure::ThreadIdentifierIsNotSet);
            };
            let Some(AttestationsTarget {
                descendant_generations: descendants_chain_length_required,
                count: attestations_target,
            }) = initial_attestations_target
            else {
                return Err(AttestationsFailure::NotAllInitialAttestationTargetsSet);
            };
            use AttestationsFailure::*;
            let Some(checkpoint) = chain.peek(descendants_chain_length_required - 1) else {
                // return Err(AttestationsFailure::ChainIsTooShort);
                continue;
            };
            let Some(checkpoint_thread_identifier) = checkpoint.thread_identifier() else {
                return Err(AttestationsFailure::ThreadIdentifierIsNotSet);
            };
            if checkpoint_thread_identifier != thread_identifier {
                if cfg!(feature = "allow-threads-merge") {
                    #[cfg(feature = "allow-threads-merge")]
                    compile_error!(
                        "it has to check if another thread is a successor of the initial block  thread."
                    );
                }
                continue;
            }
            match self.evaluate_block_attestations(
                block.block_identifier(),
                checkpoint,
                attestations_target,
            ) {
                Ok(AttestationsSuccess::InitialAttestationsTargetMet) => {
                    on_initial_attestations_target_met(block)?;
                    continue;
                }
                Ok(AttestationsSuccess::SecondaryAttestationsTargetMet) => {
                    on_secondary_attestations_target_met(block)?;
                    continue;
                }
                Err(ChainIsTooShort)
                | Err(NotAllInitialAttestationTargetsSet)
                | Err(AttestationsAreNotVerifiedYet)
                | Err(ForkResolutionsAreNotSet)
                | Err(FailedToSaveBlockState)
                | Err(ThreadIdentifierIsNotSet)
                | Err(InitialAttestationsTargetIsNotMetResolvesFork) => continue,
                Err(InvalidBlock_TailDoesNotMeetCriteria) => {
                    Err(InvalidBlock_TailDoesNotMeetCriteria)?
                }
            }
        }
    }

    fn evaluate_block_attestations(
        &self,
        block_id: &BlockIdentifier,
        initial_target: impl TargetBlock,
        min_attestations_count_required: usize,
    ) -> std::result::Result<AttestationsSuccess, AttestationsFailure> {
        let Some(fork_resolutions) = initial_target.fork_resolutions() else {
            // Must be set before proceeding
            return Err(AttestationsFailure::ForkResolutionsAreNotSet);
        };

        // Optimization assumptions:
        // - It is assumed that all attestations are folded in the last block,
        //   therefore it is possible to skip checking prev block and go straight
        //   to the initial attestation target.
        // Before this optmization it was iterating over the chain of descendants.
        let Some(block_attestations_signers) = initial_target.attestations_for(block_id) else {
            return Err(AttestationsFailure::AttestationsAreNotVerifiedYet);
        };

        // --- end of an optimization ---

        // We had all the required information to check if target was met or not.
        let is_target_met = block_attestations_signers.len() >= min_attestations_count_required;

        if is_target_met {
            return Ok(AttestationsSuccess::InitialAttestationsTargetMet);
        }
        if fork_resolutions.iter().any(|e| e.winner() == block_id) {
            // Fork resolution scenario
            if initial_target.has_attestations_target_met() {
                return Ok(AttestationsSuccess::SecondaryAttestationsTargetMet);
            }
            return Err(AttestationsFailure::InitialAttestationsTargetIsNotMetResolvesFork);
        }
        Err(AttestationsFailure::InvalidBlock_TailDoesNotMeetCriteria)
    }

    // Note: this may change with more information added in the next blocks
    pub fn find_next_block_known_dependants(
        &self,
        parent_block_identifier: BlockIdentifier,
    ) -> anyhow::Result<Vec<BlockIdentifier>> {
        let mut chain = self.prepare_chain(parent_block_identifier)?;
        let mut result = vec![];
        while !chain.is_empty() {
            let cursor = chain.remove(0);
            let (required_chain_length, block_identifier) = cursor.guarded(|e| {
                anyhow::ensure!(e.initial_attestations_target().is_some());
                Ok((
                    e.initial_attestations_target().unwrap().descendant_generations,
                    e.block_identifier().clone(),
                ))
            })?;
            if required_chain_length != chain.len() + 1 {
                continue;
            }
            result.push(block_identifier);
        }
        Ok(result)
    }

    // TODO: use select_unfinalized_ancestor_blocks instead
    // Creates a chain starting from the first non-finalized block to the tail (inclusive)
    fn prepare_chain(&self, tail: BlockIdentifier) -> anyhow::Result<Vec<BlockState>> {
        let parent_state = self.block_state_repository.get(&tail)?;
        let (is_parent_finalized, thread_id) = parent_state.guarded(|e| {
            anyhow::ensure!(!e.is_invalidated());
            anyhow::ensure!(e.thread_identifier().is_some());
            Ok((e.is_finalized(), e.thread_identifier().unwrap()))
        })?;
        if is_parent_finalized {
            return Ok(vec![]);
        }
        let (_, thread_last_finalized_block_seq_no) =
            self.repository.select_thread_last_finalized_block(&thread_id)?;
        let mut chain = vec![];
        let mut cursor = parent_state;
        loop {
            let (is_finalized, parent_id, cursor_seq_no) = cursor.guarded(|e| {
                anyhow::ensure!(!e.is_invalidated());
                if e.is_finalized() {
                    Ok((true, None, None))
                } else {
                    anyhow::ensure!(e.parent_block_identifier().is_some());
                    anyhow::ensure!(e.block_seq_no().is_some());
                    Ok((false, e.parent_block_identifier().clone(), *e.block_seq_no()))
                }
            })?;
            if is_finalized {
                chain.reverse();
                return Ok(chain);
            }
            chain.push(cursor);
            let parent_id = parent_id.unwrap();
            let cursor_seq_no = cursor_seq_no.unwrap();
            anyhow::ensure!(cursor_seq_no > thread_last_finalized_block_seq_no);
            cursor = self.block_state_repository.get(&parent_id)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn ensure_peek_returns_none_when_above_the_length() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let tmp_path = tmp_dir.path().to_owned();
        let repo = BlockStateRepository::new(tmp_path);
        let some_block_id = BlockIdentifier::from_str(
            "ffa1345a4a9ef86615040207e6f4af9f399d8f3ad4a7fc491e4e985f34c351eb",
        )
        .unwrap();
        let another_block_id = BlockIdentifier::from_str(
            "0e42bf59d3e8cad9422c9e503b4a950c625e0e662b22f1d35377d4203a3202c8",
        )
        .unwrap();
        let mut foo = VecDeque::<BlockState>::from(vec![
            repo.get(&some_block_id).unwrap(),
            repo.get(&another_block_id).unwrap(),
        ]);
        assert!(foo.peek(0).is_some());
        assert!(foo.peek(1).is_some());
        let evicted = foo.next();
        assert!(evicted.is_some());
        assert!(foo.peek(0).is_some());
        assert!(foo.peek(1).is_none());
    }
}
