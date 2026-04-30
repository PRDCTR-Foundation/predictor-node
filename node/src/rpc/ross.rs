//! ROSS RPC methods.
//!
//! ROSS = Runtime-based Optimistic State Simulation.

use std::{marker::PhantomData, sync::Arc};

use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use sc_block_builder::BlockBuilderBuilder;
use sc_client_api::{Backend as ClientBackend, StorageProvider};
use sc_transaction_pool_api::{InPoolTransaction, TransactionPool};
use serde::{Deserialize, Serialize};
use sp_api::{CallApiAt, ProvideRuntimeApi};
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_blockchain::HeaderBackend;
use sp_core::storage::StorageKey;
use sp_inherents::InherentDataProvider;
use sp_runtime::{traits::Block as BlockT, SaturatedConversion};

const MAX_STORAGE_KEYS: usize = 100;

/// Response returned by `ross_readyTxs`
#[derive(Debug, Clone, Serialize)]
pub struct ReadyTxsResponse {
    pub parent_hash: String,
    pub parent_number: u64,
    pub ready_tx_count: usize,
    pub tx_hashes: Vec<String>,
}

/// Request accepted by `ross_simulate`
#[derive(Debug, Clone, Deserialize)]
pub struct SimulateQuery {
    /// Raw storage keys to query after simulation.
    pub keys: Vec<String>,
}

/// Storage value returned after optimistic simulation
#[derive(Debug, Clone, Serialize)]
pub struct SimulatedStorageValue {
    pub key: String,
    pub changed_in_simulation: bool,
    pub value: Option<String>,
}

/// Response returned by `ross_simulate`
#[derive(Debug, Clone, Serialize)]
pub struct SimulateResponse {
    pub parent_hash: String,
    pub parent_number: u64,
    pub ready_tx_count: usize,
    pub applied_count: usize,
    pub failed_count: usize,
    pub values: Vec<SimulatedStorageValue>,
}

/// ROSS RPC API
#[rpc(server)]
pub trait RossApi {
    #[method(name = "ross_readyTxs")]
    async fn ready_txs(&self) -> RpcResult<ReadyTxsResponse>;

    #[method(name = "ross_simulate")]
    async fn simulate(&self, query: SimulateQuery) -> RpcResult<SimulateResponse>;
}

pub struct RossRpc<Client, Pool, Backend> {
    client: Arc<Client>,
    pool: Arc<Pool>,
    _backend: PhantomData<Backend>,
}

impl<Client, Pool, Backend> RossRpc<Client, Pool, Backend> {
    pub fn new(client: Arc<Client>, pool: Arc<Pool>) -> Self {
        Self { client, pool, _backend: PhantomData }
    }
}

#[async_trait::async_trait]
impl<Client, Pool, Block, Backend> RossApiServer for RossRpc<Client, Pool, Backend>
where
    Backend: ClientBackend<Block> + 'static,
    Block: BlockT,
    Client: HeaderBackend<Block>
        + sp_blockchain::HeaderMetadata<Block, Error = sp_blockchain::Error>
        + CallApiAt<Block>
        + ProvideRuntimeApi<Block>
        + StorageProvider<Block, Backend>
        + Send
        + Sync
        + 'static,
    Client::Api: BlockBuilderApi<Block>,
    Pool: TransactionPool<Block = Block> + Send + Sync + 'static,
{
    async fn ready_txs(&self) -> RpcResult<ReadyTxsResponse> {
        let info = self.client.info();
        let best_hash = info.best_hash;
        let best_number = info.best_number;

        let ready = self.pool.ready_at(best_number).await;

        let mut tx_hashes = Vec::new();
        for tx in ready {
            tx_hashes.push(format!("{:?}", tx.hash()));
        }

        Ok(ReadyTxsResponse {
            parent_hash: format!("{:?}", best_hash),
            parent_number: best_number.saturated_into::<u64>(),
            ready_tx_count: tx_hashes.len(),
            tx_hashes,
        })
    }

    async fn simulate(&self, query: SimulateQuery) -> RpcResult<SimulateResponse> {
        if query.keys.len() > MAX_STORAGE_KEYS {
            return Err(rpc_err(-32602, format!("Too many storage keys (max {MAX_STORAGE_KEYS})")))
        }

        let info = self.client.info();
        let best_hash = info.best_hash;
        let best_number = info.best_number;

        let storage_keys = query
            .keys
            .iter()
            .map(|k| decode_hex_storage_key(k))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| rpc_err(-32602, e))?;

        let ready = self.pool.ready_at(best_number).await;

        let mut builder = BlockBuilderBuilder::new(&*self.client)
            .on_parent_block(best_hash)
            .fetch_parent_block_number(&*self.client)
            .map_err(|e| rpc_err(-32000, format!("{e:?}")))?
            .build()
            .map_err(|e| rpc_err(-32001, format!("{e:?}")))?;

        let inherent_data = sp_timestamp::InherentDataProvider::from_system_time()
            .create_inherent_data()
            .await
            .map_err(|e| rpc_err(-32002, format!("{e:?}")))?;

        let inherents = builder
            .create_inherents(inherent_data)
            .map_err(|e| rpc_err(-32003, format!("{e:?}")))?;

        for inherent in inherents {
            builder.push(inherent).map_err(|e| rpc_err(-32004, format!("{e:?}")))?;
        }

        let mut applied = 0usize;
        let mut failed = 0usize;

        for tx in ready {
            match builder.push(tx.data().clone()) {
                Ok(_) => applied += 1,
                Err(_) => failed += 1,
            }
        }

        let built_block = builder.build().map_err(|e| rpc_err(-32005, format!("{e:?}")))?;

        let values = storage_keys
            .iter()
            .map(|wanted_key| {
                let simulated = built_block
                    .storage_changes
                    .main_storage_changes
                    .iter()
                    .find(|(k, _)| k.as_slice() == wanted_key.0.as_slice())
                    .map(|(_, v)| v.as_ref().map(|x| encode_hex(x.as_ref())));

                match simulated {
                    Some(v) => Ok(SimulatedStorageValue {
                        key: encode_hex(&wanted_key.0),
                        changed_in_simulation: true,
                        value: v,
                    }),
                    None => {
                        let parent = self
                            .client
                            .storage(best_hash, wanted_key)
                            .map_err(|e| rpc_err(-32006, format!("{e:?}")))?;

                        Ok(SimulatedStorageValue {
                            key: encode_hex(&wanted_key.0),
                            changed_in_simulation: false,
                            value: parent.map(|v| encode_hex(v.0.as_slice())),
                        })
                    },
                }
            })
            .collect::<RpcResult<Vec<_>>>()?;

        Ok(SimulateResponse {
            parent_hash: format!("{:?}", best_hash),
            parent_number: best_number.saturated_into::<u64>(),
            ready_tx_count: applied + failed,
            applied_count: applied,
            failed_count: failed,
            values,
        })
    }
}

fn decode_hex_storage_key(key: &str) -> Result<StorageKey, String> {
    let key = key.strip_prefix("0x").unwrap_or(key);

    if key.len() % 2 != 0 {
        return Err("invalid hex length".into())
    }

    let bytes = (0..key.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&key[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{e:?}"))?;

    Ok(StorageKey(bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::from("0x");
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn rpc_err(code: i32, msg: impl Into<String>) -> jsonrpsee::types::ErrorObjectOwned {
    jsonrpsee::types::ErrorObjectOwned::owned(code, msg.into(), None::<()>)
}
