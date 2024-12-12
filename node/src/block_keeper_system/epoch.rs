// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use std::str::FromStr;

use num_bigint::BigUint;
use num_traits::Zero;
use tvm_abi::TokenValue;
use tvm_block::Account;
use tvm_block::ExternalInboundMessageHeader;
use tvm_block::Message;
use tvm_block::MsgAddressInt;
use tvm_client::encoding::slice_from_cell;
use tvm_types::SliceData;

use crate::block_keeper_system::abi::EPOCH_ABI;
use crate::block_keeper_system::BlockKeeperData;
use crate::block_keeper_system::BlockKeeperStatus;
use crate::bls::gosh_bls::PubKey;
use crate::node::SignerIndex;

const BLS_PUBKEY_TOKEN_KEY: &str = "_bls_pubkey";
const WALLET_ID_TOKEN_KEY: &str = "_walletId";
const EPOCH_FINISH_TOKEN_KEY: &str = "_unixtimeFinish";
const STAKE_KEY: &str = "_stake";

fn get_epoch_abi() -> tvm_client::abi::Abi {
    tvm_client::abi::Abi::Json(EPOCH_ABI.to_string())
}

pub fn decode_epoch_data(
    account: &Account,
) -> anyhow::Result<Option<(SignerIndex, BlockKeeperData)>> {
    let abi = get_epoch_abi();
    if let Some(data) = account.get_data() {
        let decoded_data = abi
            .abi()
            .map_err(|e| anyhow::format_err!("Failed to load epoch ABI: {e}"))?
            .decode_storage_fields(
                slice_from_cell(data)
                    .map_err(|e| anyhow::format_err!("Failed to decode epoch data cell: {e}"))?,
                true,
            )
            .map_err(|e| anyhow::format_err!("Failed to decode epoch storage: {e}"))?;
        let mut block_keeper_bls_key = None;
        let mut block_keeper_wallet_id = None;
        let mut block_keeper_epoch_finish = None;
        let mut block_keeper_stake = None;
        for token in decoded_data {
            if token.name == BLS_PUBKEY_TOKEN_KEY {
                if let TokenValue::Bytes(pubkey) = token.value {
                    block_keeper_bls_key = Some(PubKey::from(pubkey.as_slice()));
                }
            } else if token.name == WALLET_ID_TOKEN_KEY {
                if let TokenValue::Uint(wallet_id) = token.value {
                    // TODO: check that wallet id fits boundaries
                    tracing::trace!("decoded epoch wallet id: {wallet_id:?}");
                    block_keeper_wallet_id = Some(if wallet_id.number.is_zero() {
                        0
                    } else {
                        wallet_id.number.to_u64_digits()[0] as SignerIndex
                    });
                }
            } else if token.name == EPOCH_FINISH_TOKEN_KEY {
                if let TokenValue::Uint(epoch_finish) = token.value {
                    tracing::trace!("decoded epoch finish: {epoch_finish:?}");
                    block_keeper_epoch_finish = Some(if epoch_finish.number.is_zero() {
                        0
                    } else {
                        epoch_finish.number.to_u32_digits()[0]
                    });
                }
            } else if token.name == STAKE_KEY {
                if let TokenValue::Uint(stake) = token.value {
                    tracing::trace!("decoded epoch stake: {stake:?}");
                    block_keeper_stake =
                        Some(if stake.number.is_zero() { BigUint::zero() } else { stake.number });
                }
            }
        }
        if block_keeper_bls_key.is_some()
            && block_keeper_wallet_id.is_some()
            && block_keeper_epoch_finish.is_some()
            && block_keeper_stake.is_some()
        {
            let index = block_keeper_wallet_id.unwrap();
            return Ok(Some((
                index,
                BlockKeeperData {
                    index,
                    pubkey: block_keeper_bls_key.unwrap(),
                    epoch_finish_timestamp: block_keeper_epoch_finish.unwrap(),
                    status: BlockKeeperStatus::Active,
                    address: account.get_addr().unwrap().to_string(),
                    stake: block_keeper_stake.unwrap(),
                },
            )));
        }
    }
    Ok(None)
}

pub fn create_epoch_touch_message(data: &BlockKeeperData, time: u32) -> anyhow::Result<Message> {
    tracing::trace!("create_epoch_touch_message: {data:?}");
    let expire = time + 5;
    let msg_body = tvm_abi::encode_function_call(
        EPOCH_ABI,
        "touch",
        Some(&format!(r#"{{"expire":{}}}"#, expire)),
        "{}",
        false,
        None,
        Some(&data.address),
    )
    .map_err(|e| anyhow::format_err!("Failed to create message body: {e}"))?;
    let header = ExternalInboundMessageHeader {
        dst: MsgAddressInt::from_str(&data.address)
            .map_err(|e| anyhow::format_err!("Failed to generate epoch address: {e}"))?,
        ..Default::default()
    };
    let body = SliceData::load_cell(
        msg_body
            .into_cell()
            .map_err(|e| anyhow::format_err!("Failed serialize message body: {e}"))?,
    )
    .map_err(|e| anyhow::format_err!("Failed to serialize message body: {e}"))?;
    Ok(Message::with_ext_in_header_and_body(header, body))
}
