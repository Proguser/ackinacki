// 2022-2024 (c) Copyright Contributors to the GOSH DAO. All rights reserved.
//

use account::BlockchainAccountQuery;
use account::BlockchainMasterSeqNoFilter;
use async_graphql::connection::query;
use async_graphql::connection::Connection;
use async_graphql::connection::Edge;
use async_graphql::connection::EmptyFields;
use async_graphql::dataloader::DataLoader;
use async_graphql::Context;
use async_graphql::Object;
use blocks::BlockchainBlock;
use blocks::BlockchainBlocksConnection;
use blocks::BlockchainBlocksEdge;
use blocks::BlockchainBlocksQueryArgs;
use query::PaginateDirection;
use query::QueryArgs;
use sqlx::SqlitePool;
use transactions::BlockchainMessage;
use transactions::BlockchainTransaction;
use transactions::BlockchainTransactionsConnection;
use transactions::BlockchainTransactionsEdge;
use transactions::BlockchainTransactionsQueryArgs;

use super::block::BlockLoader;
use super::message::MessageLoader;
use super::transaction::TransactionLoader;
use crate::schema::db;
use crate::schema::graphql;

pub mod account;
pub mod blocks;
pub mod query;
pub mod transactions;

/// Blockchain-related information (blocks, transactions, etc.).
pub struct BlockchainQuery<'a> {
    pub ctx: &'a Context<'a>,
}

#[Object]
impl BlockchainQuery<'_> {
    #[graphql(name = "account")]
    /// Account-related information.
    async fn account(&self, address: String) -> Option<BlockchainAccountQuery> {
        Some(BlockchainAccountQuery { ctx: self.ctx, address })
    }

    async fn block(&self, hash: String) -> Option<BlockchainBlock> {
        let block_loader = self.ctx.data_unchecked::<DataLoader<BlockLoader>>();
        let message_loader = self.ctx.data_unchecked::<DataLoader<MessageLoader>>();
        let block = block_loader.load_one(hash).await.expect("Failed to load block");

        block.as_ref()?;

        let block = block.unwrap();

        // if self.ctx.look_ahead().field("block").field("in_message").exists() {
        //     let in_message =
        //         message_loader.load_one(block.in_msg.clone()).await.expect("Failed to
        // load in_message");     block.in_message = in_message;
        // }

        if self.ctx.look_ahead().field("block").field("out_messages").exists() {
            let out_msg_ids = block.out_msgs.clone();
            let _out_messages =
                message_loader.load_many(out_msg_ids).await.expect("Failed to load out_messages");

            // block.out_msg_descr =
            //     Some(out_messages.into_values().map(|m| Some(m)).collect());
        }

        Some(block)
    }

    #[allow(clippy::too_many_arguments)]
    /// This node could be used for a cursor-based pagination of blocks.
    async fn blocks(
        &self,
        #[graphql(
            name = "allow_latest_inconsistent_data",
            desc = "By default there is special latency added for the fetched recent data (several seconds) to ensure impossibility of inserts before the latest fetched cursor (data consistency, for reliable pagination). It is possible to disable this guarantee and to reduce the latency of realtime data by setting this flag to true."
        )]
        _allow_latest_inconsistent_data: Option<bool>,
        #[graphql(name = "master_seq_no_range")] block_seq_no_range: Option<
            BlockchainMasterSeqNoFilter,
        >,
        #[graphql(
            name = "min_tr_count",
            desc = "Optional filter by minimum transactions in a block (unoptimized, query could be dropped by timeout)"
        )]
        min_tr_count: Option<i32>,
        #[graphql(
            name = "max_tr_count",
            desc = "Optional filter by maximum transactions in a block (unoptimized, query could be dropped by timeout)"
        )]
        max_tr_count: Option<i32>,
        #[graphql(desc = "This field is mutually exclusive with 'last'.")] first: Option<i32>,
        after: Option<String>,
        #[graphql(desc = "This field is mutually exclusive with 'first'.")] last: Option<i32>,
        before: Option<String>,
    ) -> Option<
        Connection<
            String,
            BlockchainBlock,
            EmptyFields,
            EmptyFields,
            BlockchainBlocksConnection,
            BlockchainBlocksEdge,
        >,
    > {
        query(after, before, first, last, |after, before, first, last| async move {
            let args = BlockchainBlocksQueryArgs {
                block_seq_no_range,
                min_tr_count,
                max_tr_count,
                first,
                after: after.clone(),
                last,
                before: before.clone(),
            };
            let mut blocks: Vec<db::Block> = db::block::Block::blockchain_blocks(
                self.ctx.data::<SqlitePool>().unwrap(),
                args.clone(),
            )
            .await?;

            let (has_previous_page, has_next_page) =
                calc_prev_next_markers(after, before, first, last, blocks.len());
            log::debug!("has_previous_page={:?}, after={:?}", has_previous_page, has_next_page);

            let mut connection: Connection<
                String,
                graphql::Block,
                EmptyFields,
                EmptyFields,
                BlockchainBlocksConnection,
                BlockchainBlocksEdge,
            > = Connection::new(has_previous_page, has_next_page);

            if blocks.len() >= args.get_limit() {
                match args.get_direction() {
                    PaginateDirection::Forward => blocks.truncate(blocks.len() - 1),
                    PaginateDirection::Backward => {
                        blocks.drain(0..1);
                    }
                }
            }

            connection.edges.extend(blocks.into_iter().map(|block| {
                let mut block: BlockchainBlock = block.into();
                block.id = format!("block/{}", block.id);
                // eprintln!("{block:?}");
                let cursor = block.chain_order.clone().unwrap();
                let edge: Edge<String, graphql::block::Block, EmptyFields, BlockchainBlocksEdge> =
                    Edge::with_additional_fields(cursor, block, EmptyFields);
                edge
            }));

            Ok::<_, async_graphql::Error>(connection)
        })
        .await
        .ok()
    }

    async fn message(&self, hash: String) -> Option<BlockchainMessage> {
        let transaction_loader = self.ctx.data_unchecked::<DataLoader<TransactionLoader>>();
        let message_loader = self.ctx.data_unchecked::<DataLoader<MessageLoader>>();

        let message = message_loader.load_one(hash).await.expect("Failed to load message");
        message.as_ref()?;

        let mut message = message.unwrap();
        if self.ctx.look_ahead().field("message").field("src_transaction").exists() {
            if let Some(transaction_id) = message.transaction_id.clone() {
                message.src_transaction = transaction_loader
                    .load_one(transaction_id.clone())
                    .await
                    .unwrap_or_else(|_| panic!("Failed to load transaction: {transaction_id}"))
                    .map(Box::new);
            }
        }

        if self.ctx.look_ahead().field("message").field("dst_transaction").exists() {
            let dst_transaction = db::transaction::Transaction::by_in_message(
                self.ctx.data::<SqlitePool>().unwrap(),
                &message.id,
                None,
            )
            .await
            .expect("Failed to load transaction by inbound message");

            if let Some(transaction) = dst_transaction {
                message.dst_transaction = transaction_loader
                    .load_one(transaction.id.clone())
                    .await
                    .unwrap_or_else(|_| panic!("Failed to load transaction: {}", transaction.id))
                    .map(Box::new);
            }
        }

        Some(message)
    }

    async fn transaction(&self, hash: String) -> Option<BlockchainTransaction> {
        let transaction_loader = self.ctx.data_unchecked::<DataLoader<TransactionLoader>>();
        let message_loader = self.ctx.data_unchecked::<DataLoader<MessageLoader>>();
        let transaction =
            transaction_loader.load_one(hash).await.expect("Failed to load transaction");

        transaction.as_ref()?;

        let mut transaction = transaction.unwrap();

        if self.ctx.look_ahead().field("transaction").field("in_message").exists() {
            let in_message = message_loader
                .load_one(transaction.in_msg.clone())
                .await
                .expect("Failed to load in_message");
            transaction.in_message = in_message;
        }

        if self.ctx.look_ahead().field("transaction").field("out_messages").exists() {
            let out_msg_ids = transaction.out_msgs.clone();
            let out_messages =
                message_loader.load_many(out_msg_ids).await.expect("Failed to load out_messages");

            transaction.out_messages = Some(out_messages.into_values().map(Some).collect());
        }

        Some(transaction)
    }

    #[allow(clippy::too_many_arguments)]
    /// This node could be used for a cursor-based pagination of transactions.
    async fn transactions(
        &self,
        #[graphql(
            name = "allow_latest_inconsistent_data",
            desc = "By default there is special latency added for the fetched recent data (several seconds) to ensure impossibility of inserts before the latest fetched cursor (data consistency, for reliable pagination). It is possible to disable this guarantee and to reduce the latency of realtime data by setting this flag to true."
        )]
        _allow_latest_inconsistent_data: Option<bool>,
        #[graphql(
            name = "min_balance_delta",
            desc = "Optional filter by min balance_delta (unoptimized, query could be dropped by timeout)."
        )]
        min_balance_delta: Option<String>,
        #[graphql(
            name = "max_balance_delta",
            desc = "Optional filter by max balance_delta (unoptimized, query could be dropped by timeout)."
        )]
        max_balance_delta: Option<String>,
        #[graphql(
            name = "code_hash",
            desc = "Optional filter by code hash of the account before execution."
        )]
        code_hash: Option<String>,
        #[graphql(desc = "This field is mutually exclusive with 'last'.")] first: Option<i32>,
        after: Option<String>,
        #[graphql(desc = "This field is mutually exclusive with 'first'.")] last: Option<i32>,
        before: Option<String>,
    ) -> Option<
        Connection<
            String,
            BlockchainTransaction,
            EmptyFields,
            EmptyFields,
            BlockchainTransactionsConnection,
            BlockchainTransactionsEdge,
        >,
    > {
        query(
            after,
            before,
            first,
            last,
            |after: Option<String>, before: Option<String>, first, last| async move {
                let args = BlockchainTransactionsQueryArgs {
                    min_balance_delta,
                    max_balance_delta,
                    code_hash,
                    first,
                    after: after.clone(),
                    last,
                    before: before.clone(),
                };
                let message_loader = self.ctx.data_unchecked::<DataLoader<MessageLoader>>();
                let mut transactions = db::transaction::Transaction::blockchain_transactions(
                    self.ctx.data::<SqlitePool>().unwrap(),
                    args.clone(),
                )
                .await?;

                let (has_previous_page, has_next_page) =
                    calc_prev_next_markers(after, before, first, last, transactions.len());
                log::debug!("has_previous_page={:?}, after={:?}", has_previous_page, has_next_page);

                let mut connection: Connection<
                    String,
                    graphql::Transaction,
                    EmptyFields,
                    EmptyFields,
                    BlockchainTransactionsConnection,
                    BlockchainTransactionsEdge,
                > = Connection::new(has_previous_page, has_next_page);

                if transactions.len() >= args.get_limit() {
                    match args.get_direction() {
                        PaginateDirection::Forward => transactions.truncate(transactions.len() - 1),
                        PaginateDirection::Backward => {
                            transactions.drain(0..1);
                        }
                    }
                }

                let selection_set =
                    self.ctx.look_ahead().field("transactions").field("edges").field("node");
                let mut edges = Vec::new();
                for transaction in transactions.into_iter() {
                    let mut transaction: BlockchainTransaction = transaction.into();

                    if selection_set.field("in_message").exists() {
                        let in_message =
                            message_loader.load_many(vec![transaction.in_msg.clone()]).await?;
                        eprintln!("in_message: {in_message:?}");
                        transaction.in_message =
                            in_message.get(&transaction.in_msg).map(ToOwned::to_owned);
                    }

                    if selection_set.field("out_messages").exists() {
                        let out_msg_ids = transaction.out_msgs.clone();
                        let out_messages = message_loader.load_many(out_msg_ids).await?;
                        transaction.out_messages =
                            Some(out_messages.into_values().map(Some).collect());
                    }

                    let cursor = transaction.chain_order.clone();
                    let edge: Edge<
                        String,
                        graphql::transaction::Transaction,
                        EmptyFields,
                        BlockchainTransactionsEdge,
                    > = Edge::with_additional_fields(cursor, transaction, EmptyFields);
                    edges.push(edge);
                }

                connection.edges.extend(edges);

                Ok::<_, async_graphql::Error>(connection)
            },
        )
        .await
        .ok()
    }
}

fn calc_prev_next_markers(
    after: Option<String>,
    before: Option<String>,
    first: Option<usize>,
    last: Option<usize>,
    num_nodes: usize,
) -> (bool, bool) {
    let has_prev_page =
        if let Some(last) = last { num_nodes > last || after.is_some() } else { false };
    let has_next_page =
        if let Some(first) = first { num_nodes > first || before.is_some() } else { false };
    (has_prev_page, has_next_page)
}
