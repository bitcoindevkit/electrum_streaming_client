//! Types representing structured responses returned by the Electrum server.
//!
//! This module defines deserializable Rust types that correspond to the return values of various
//! Electrum JSON-RPC methods. These types are used to decode responses for specific request types
//! defined in the [`crate::request`] module.

use std::collections::HashMap;

use bitcoin::{
    absolute,
    hashes::{Hash, HashEngine},
    Amount, BlockHash,
};

use crate::DoubleSHA;

/// Response to the `"blockchain.block.header"` method (without checkpoint).
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct HeaderResp {
    /// The block header at the requested height.
    #[serde(deserialize_with = "crate::custom_serde::from_consensus_hex")]
    pub header: bitcoin::block::Header,
}

/// Response to the `"blockchain.block.header"` method with a `cp_height` parameter.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct HeaderWithProofResp {
    /// A Merkle branch connecting the header to the provided checkpoint root.
    pub branch: Vec<DoubleSHA>,

    /// The block header at the requested height.
    #[serde(deserialize_with = "crate::custom_serde::from_consensus_hex")]
    pub header: bitcoin::block::Header,

    /// The Merkle root for the header chain up to the checkpoint height.
    pub root: DoubleSHA,
}

/// Response to the `"blockchain.block.headers"` method (without checkpoint).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeadersResp {
    /// The number of headers returned.
    pub count: usize,

    /// The deserialized headers returned by the server.
    #[serde(
        rename = "hex",
        deserialize_with = "crate::custom_serde::from_cancat_consensus_hex"
    )]
    pub headers: Vec<bitcoin::block::Header>,

    /// The server’s maximum allowed headers per request.
    pub max: usize,
}

/// Response to the `"blockchain.block.headers"` method with a `cp_height` parameter.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeadersWithCheckpointResp {
    /// The number of headers returned.
    pub count: usize,

    /// The deserialized headers returned by the server.
    #[serde(
        rename = "hex",
        deserialize_with = "crate::custom_serde::from_cancat_consensus_hex"
    )]
    pub headers: Vec<bitcoin::block::Header>,

    /// The server’s maximum allowed headers per request.
    pub max: usize,

    /// The Merkle root of all headers up to the checkpoint height.
    pub root: DoubleSHA,

    /// A Merkle branch proving inclusion of the last header in the checkpoint root.
    pub branch: Vec<DoubleSHA>,
}

/// Response to the `"blockchain.estimatefee"` method.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct EstimateFeeResp {
    /// The estimated fee rate, or `None` if the server could not estimate.
    #[serde(deserialize_with = "crate::custom_serde::feerate_opt_from_btc_per_kb")]
    pub fee_rate: Option<bitcoin::FeeRate>,
}

/// Response to the `"blockchain.headers.subscribe"` method.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct HeadersSubscribeResp {
    /// The latest block header known to the server.
    #[serde(
        rename = "hex",
        deserialize_with = "crate::custom_serde::from_consensus_hex"
    )]
    pub header: bitcoin::block::Header,

    /// The height of the block in the header.
    pub height: u32,
}

/// Response to the `"server.relayfee"` method.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct RelayFeeResp {
    /// The minimum fee amount that the server will accept for relaying transactions.
    #[serde(deserialize_with = "crate::custom_serde::amount_from_btc")]
    pub fee: Amount,
}

/// Response to the `"blockchain.scripthash.get_balance"` method.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GetBalanceResp {
    /// The confirmed balance in satoshis.
    #[serde(deserialize_with = "crate::custom_serde::amount_from_sats")]
    pub confirmed: Amount,

    /// The unconfirmed balance in satoshis (may be negative).
    #[serde(deserialize_with = "crate::custom_serde::amount_from_maybe_negative_sats")]
    pub unconfirmed: Amount,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum Tx {
    Mempool(MempoolTx),
    Confirmed(ConfirmedTx),
}

impl Tx {
    pub fn txid(&self) -> bitcoin::Txid {
        match self {
            Tx::Mempool(MempoolTx { txid, .. }) => *txid,
            Tx::Confirmed(ConfirmedTx { txid, .. }) => *txid,
        }
    }

    pub fn confirmation_height(&self) -> Option<absolute::Height> {
        match self {
            Tx::Mempool(_) => None,
            Tx::Confirmed(ConfirmedTx { height, .. }) => Some(*height),
        }
    }

    /// Returns the transaction height as represented by the Electrum API.
    ///
    /// * Confirmed transactions have a height > 0.
    /// * Unconfirmed transactions either have a height of 0 or -1.
    ///   * 0 means transaction inputs are all confirmed.
    ///   * -1 means not all transaction inputs are confirmed.
    pub fn electrum_height(&self) -> i64 {
        match self {
            Tx::Mempool(mempool_tx) if mempool_tx.confirmed_inputs => 0,
            Tx::Mempool(_) => -1,
            Tx::Confirmed(confirmed_tx) => confirmed_tx.height.to_consensus_u32() as i64,
        }
    }
}

/// A confirmed transaction entry returned by `"blockchain.scripthash.get_history"`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConfirmedTx {
    /// The transaction ID.
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,

    /// The height of the block containing this transaction.
    pub height: absolute::Height,
}

/// An unconfirmed transaction returned by `"blockchain.scripthash.get_mempool"`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MempoolTx {
    /// The transaction ID.
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,

    /// The fee paid by the transaction in satoshis.
    #[serde(deserialize_with = "crate::custom_serde::amount_from_sats")]
    pub fee: bitcoin::Amount,

    /// Whether all inputs are confirmed.
    #[serde(
        rename = "height",
        deserialize_with = "crate::custom_serde::all_inputs_confirmed_bool_from_height"
    )]
    pub confirmed_inputs: bool,
}

/// Response entry from the `"blockchain.scripthash.listunspent"` method.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Utxo {
    /// The height of the block in which the UTXO was confirmed, or `0` if unconfirmed.
    pub height: absolute::Height,

    /// The output index of the transaction.
    pub tx_pos: usize,

    /// The transaction ID that created this UTXO.
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,

    /// The value of the UTXO in satoshis.
    #[serde(deserialize_with = "crate::custom_serde::amount_from_sats")]
    pub value: bitcoin::Amount,
}

/// Response to the `"blockchain.transaction.get"` method.
///
/// Contains the full deserialized transaction.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct FullTx {
    /// The full transaction.
    #[serde(deserialize_with = "crate::custom_serde::from_consensus_hex")]
    pub tx: bitcoin::Transaction,
}

/// Response to the `"blockchain.transaction.get_merkle"` method.
///
/// Contains a Merkle proof of inclusion in a block.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TxMerkle {
    /// The height of the block containing the transaction.
    pub block_height: absolute::Height,

    /// The Merkle branch connecting the transaction to the block root.
    pub merkle: Vec<DoubleSHA>,

    /// The transaction's position in the block's Merkle tree.
    pub pos: usize,
}

impl TxMerkle {
    /// Returns the merkle root of a [`Header`] which satisfies this proof.
    ///
    /// [`Header`]: bitcoin::block::Header
    pub fn expected_merkle_root(&self, txid: bitcoin::Txid) -> bitcoin::TxMerkleNode {
        let mut index = self.pos;
        let mut cur = txid.to_raw_hash();
        for next_hash in &self.merkle {
            cur = DoubleSHA::from_engine({
                let mut engine = DoubleSHA::engine();
                if index % 2 == 0 {
                    engine.input(cur.as_ref());
                    engine.input(next_hash.as_ref());
                } else {
                    engine.input(next_hash.as_ref());
                    engine.input(cur.as_ref());
                };
                engine
            });
            index /= 2;
        }
        cur.into()
    }
}

/// Response to the `"blockchain.transaction.id_from_pos"` method.
///
/// Returns the transaction ID at the given position in a block.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct TxidFromPos {
    /// The transaction ID located at the specified position.
    pub txid: bitcoin::Txid,
}

/// Response entry from the `"mempool.get_fee_histogram"` method.
///
/// Describes one fee-rate bin and the total weight of transactions at or above that rate.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FeePair {
    /// The minimum fee rate (in sat/vB) for this bucket.
    #[serde(deserialize_with = "crate::custom_serde::feerate_from_sat_per_byte")]
    pub fee_rate: bitcoin::FeeRate,

    /// The total weight (in vbytes) of transactions at or above this fee rate.
    #[serde(deserialize_with = "crate::custom_serde::weight_from_vb")]
    pub weight: bitcoin::Weight,
}

/// Response to the `"server.features"` method.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ServerFeatures {
    /// Hosts.
    pub hosts: HashMap<String, ServerHostValues>,

    /// The hash of the genesis block.
    ///
    ///  This is used to detect if a peer is connected to one serving a different network.
    pub genesis_hash: BlockHash,

    /// The hash function the server uses for script hashing.
    ///
    /// The default is `"sha-256"`.
    pub hash_function: String,

    /// A string that identifies the server software.
    pub server_version: String,

    /// The max protocol version.
    pub protocol_max: String,

    /// The min protocol version.
    pub protocol_min: String,

    /// The pruning limit.
    pub pruning: Option<u32>,
}

/// Server host values.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ServerHostValues {
    /// SSL Port.
    pub ssl_port: Option<u16>,
    /// TCP Port.
    pub tcp_port: Option<u16>,
}
