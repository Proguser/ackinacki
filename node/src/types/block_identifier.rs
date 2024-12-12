// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use serde_with::Bytes;

#[serde_as]
#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Default)]
pub struct BlockIdentifier(#[serde_as(as = "Bytes")] [u8; 32]);

impl BlockIdentifier {
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    /// This is not a modulo operation.
    /// It is some kind of a quick and dirty way to put blocks into a defined
    /// number of buckets.
    pub fn not_a_modulus(&self, divider: u32) -> u32 {
        let mut bytes = [0_u8; 4];
        bytes.clone_from_slice(&self.0[28..=31]);
        u32::from_be_bytes(bytes) % divider
    }
}

impl AsRef<[u8]> for BlockIdentifier {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<tvm_types::UInt256> for BlockIdentifier {
    fn from(value: tvm_types::UInt256) -> Self {
        Self(value.inner())
    }
}

impl From<BlockIdentifier> for tvm_types::UInt256 {
    fn from(val: BlockIdentifier) -> Self {
        tvm_types::UInt256::from(val.0)
    }
}

impl std::str::FromStr for BlockIdentifier {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut result = [0u8; 32];
        match value.len() {
            64 => hex::decode_to_slice(value, &mut result)?,
            66 => hex::decode_to_slice(&value[2..], &mut result)?,
            44 => tvm_types::base64_decode_to_slice(value, &mut result).map_err(|e| {
                anyhow::format_err!("Failed to decode UInt256 from base64 str: {e}")
            })?,
            _ => anyhow::bail!(
                "invalid account ID string (32 bytes expected), but got string {}",
                value
            ),
        }
        Ok(Self(result))
    }
}

impl std::fmt::Debug for BlockIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::LowerHex::fmt(self, f)
    }
}

impl std::fmt::LowerHex for BlockIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "0x{}", hex::encode(self.0))
        } else {
            write!(f, "{}", hex::encode(self.0))
            // write!(f, "{}...{}", hex::encode(&self.0[..2]),
            // hex::encode(&self.0[30..32]))
        }
    }
}

impl std::fmt::Display for BlockIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            // "UInt256[{:X?}]", &self.0  This format is better for debug. It was used in path
            // creation and this wrap looks bad
            "{}",
            hex::encode(self.0)
        )
    }
}
