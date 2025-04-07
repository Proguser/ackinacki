use std::cmp::Ordering;

use derive_getters::Getters;

use crate::bls::envelope::BLSSignedEnvelope;
use crate::bls::envelope::Envelope;
use crate::bls::GoshBLS;
use crate::types::AckiNackiBlock;
use crate::types::BlockIdentifier;
use crate::types::BlockSeqNo;

#[derive(Debug, Clone, Eq, PartialEq, Getters)]
pub struct BlockIndex {
    block_seq_no: BlockSeqNo,
    block_identifier: BlockIdentifier,
}

impl From<&Envelope<GoshBLS, AckiNackiBlock>> for BlockIndex {
    fn from(acki_block: &Envelope<GoshBLS, AckiNackiBlock>) -> Self {
        Self {
            block_seq_no: acki_block.data().seq_no(),
            block_identifier: acki_block.data().identifier().clone(),
        }
    }
}

impl PartialOrd for BlockIndex {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.block_seq_no.cmp(&other.block_seq_no))
    }
}

impl Ord for BlockIndex {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.block_seq_no.cmp(&other.block_seq_no) {
            Ordering::Equal => {
                BlockIdentifier::compare(&self.block_identifier, &other.block_identifier)
            }
            ordering => ordering,
        }
    }
}
