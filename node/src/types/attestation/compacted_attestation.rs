use std::collections::BTreeMap;
use std::collections::HashMap;

use crate::bls::envelope::BLSSignedEnvelope;
use crate::bls::envelope::Envelope;
use crate::bls::BLSSignatureScheme;
use crate::bls::GoshBLS;
use crate::node::associated_types::AttestationData;
use crate::node::SignerIndex;
use crate::types::attestation::compacted_map_key::CompactedMapKey;
use crate::types::BlockIdentifier;

#[derive(Hash, PartialEq, Clone, Eq)]
pub struct CompactedAttestation {
    parent_block_id: BlockIdentifier,
    aggregated_signature: <GoshBLS as BLSSignatureScheme>::Signature,
    signature_occurrences: BTreeMap<SignerIndex, u16>,
}

impl From<&Envelope<GoshBLS, AttestationData>> for CompactedAttestation {
    fn from(value: &Envelope<GoshBLS, AttestationData>) -> Self {
        CompactedAttestation {
            parent_block_id: value.data().parent_block_id().clone(),
            aggregated_signature: value.aggregated_signature().clone(),
            signature_occurrences: BTreeMap::from_iter(value.clone_signature_occurrences()),
        }
    }
}

impl From<(CompactedAttestation, &CompactedMapKey)> for Envelope<GoshBLS, AttestationData> {
    fn from(value: (CompactedAttestation, &CompactedMapKey)) -> Self {
        Envelope::create(
            value.0.aggregated_signature,
            HashMap::from_iter(value.0.signature_occurrences),
            AttestationData::builder()
                .block_id(value.1.block_identifier().clone())
                .block_seq_no(*value.1.block_seq_no())
                .parent_block_id(value.0.parent_block_id.clone())
                .build(),
        )
    }
}
