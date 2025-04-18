use std::fmt::Debug;
use std::fmt::Formatter;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use tvm_block::Deserializable;
use tvm_block::Serializable;
use tvm_types::UInt256;

#[derive(Clone, PartialEq)]
pub struct WrappedAccount {
    pub account_id: UInt256,
    pub account: tvm_block::ShardAccount,
    pub aug: tvm_block::DepthBalanceInfo,
}

impl Debug for WrappedAccount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.account_id.to_hex_string())
    }
}

#[derive(Serialize, Deserialize)]
struct WrappedAccountData {
    account_id: [u8; 32],
    data: Vec<u8>,
    aug: Vec<u8>,
}

impl WrappedAccount {
    fn wrap_serialize(&self) -> WrappedAccountData {
        WrappedAccountData {
            account_id: self.account_id.clone().inner(),
            data: self.account.write_to_bytes().unwrap(),
            aug: self.aug.write_to_bytes().unwrap(),
        }
    }

    fn wrap_deserialize(data: WrappedAccountData) -> Self {
        Self {
            account_id: UInt256::from(data.account_id),
            account: tvm_block::ShardAccount::construct_from_bytes(&data.data).unwrap(),
            aug: tvm_block::DepthBalanceInfo::construct_from_bytes(&data.aug).unwrap(),
        }
    }
}

impl Serialize for WrappedAccount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.wrap_serialize().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WrappedAccount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = WrappedAccountData::deserialize(deserializer)?;
        let account = WrappedAccount::wrap_deserialize(data);
        Ok(account)
    }
}
