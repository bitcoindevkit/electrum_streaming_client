//! Types representing structured responses returned by the Electrum server.
//!
//! This module defines Rust types that correspond to the return values of various Electrum
//! JSON-RPC methods. These types are used to decode responses for specific request types defined in
//! the [`crate::request`] module.
//!
//! Every type here also implements [`serde::Serialize`] so that responses can be cached or
//! persisted. Serialization is symmetric with deserialization: the emitted JSON is the Electrum
//! wire representation, so it can be fed straight back into [`serde::Deserialize`].

use std::collections::HashMap;

use bitcoin::{
    absolute,
    hashes::{Hash, HashEngine},
    Amount, BlockHash, SignedAmount,
};

use crate::DoubleSHA;

/// Response to the `"server.version"` method.
///
/// Returns the server's software version and the negotiated protocol version.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#server-version>
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(from = "(String, String)", into = "(String, String)")]
pub struct ServerVersionResp {
    /// Server software version (e.g. `"ElectrumX 1.18.0"`).
    pub server_software: String,

    /// Negotiated protocol version (e.g. `"1.4"`).
    pub protocol_version: String,
}

impl From<ServerVersionResp> for (String, String) {
    fn from(resp: ServerVersionResp) -> Self {
        (resp.server_software, resp.protocol_version)
    }
}

impl From<(String, String)> for ServerVersionResp {
    fn from((server_software, protocol_version): (String, String)) -> Self {
        Self {
            server_software,
            protocol_version,
        }
    }
}

/// Response to the `"blockchain.block.header"` method (without checkpoint).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct HeaderResp {
    /// The block header at the requested height.
    #[serde(
        deserialize_with = "crate::custom_serde::from_consensus_hex",
        serialize_with = "crate::custom_serde::to_consensus_hex"
    )]
    pub header: bitcoin::block::Header,
}

/// Response to the `"blockchain.block.header"` method with a `cp_height` parameter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HeaderWithProofResp {
    /// A Merkle branch connecting the header to the provided checkpoint root.
    pub branch: Vec<DoubleSHA>,

    /// The block header at the requested height.
    #[serde(
        deserialize_with = "crate::custom_serde::from_consensus_hex",
        serialize_with = "crate::custom_serde::to_consensus_hex"
    )]
    pub header: bitcoin::block::Header,

    /// The Merkle root for the header chain up to the checkpoint height.
    pub root: DoubleSHA,
}

/// Response to the `"blockchain.block.headers"` method (without checkpoint).
///
/// Supports both the pre-1.6 format (concatenated hex in `"hex"` field) and the v1.6 format
/// (array of hex strings in `"headers"` field).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HeadersResp {
    /// The number of headers returned.
    pub count: usize,

    /// The deserialized headers returned by the server.
    #[serde(
        alias = "hex",
        alias = "headers",
        deserialize_with = "crate::custom_serde::headers_from_hex_or_list",
        serialize_with = "crate::custom_serde::headers_to_hex_list"
    )]
    pub headers: Vec<bitcoin::block::Header>,

    /// The server's maximum allowed headers per request.
    pub max: usize,
}

/// Response to the `"blockchain.block.headers"` method with a `cp_height` parameter.
///
/// Supports both the pre-1.6 format (concatenated hex in `"hex"` field) and the v1.6 format
/// (array of hex strings in `"headers"` field).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HeadersWithCheckpointResp {
    /// The number of headers returned.
    pub count: usize,

    /// The deserialized headers returned by the server.
    #[serde(
        alias = "hex",
        alias = "headers",
        deserialize_with = "crate::custom_serde::headers_from_hex_or_list",
        serialize_with = "crate::custom_serde::headers_to_hex_list"
    )]
    pub headers: Vec<bitcoin::block::Header>,

    /// The server's maximum allowed headers per request.
    pub max: usize,

    /// The Merkle root of all headers up to the checkpoint height.
    pub root: DoubleSHA,

    /// A Merkle branch proving inclusion of the last header in the checkpoint root.
    pub branch: Vec<DoubleSHA>,
}

/// Response to the `"blockchain.estimatefee"` method.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct EstimateFeeResp {
    /// The estimated fee rate, or `None` if the server could not estimate.
    #[serde(
        deserialize_with = "crate::custom_serde::feerate_opt_from_btc_per_kb",
        serialize_with = "crate::custom_serde::feerate_opt_to_btc_per_kb"
    )]
    pub fee_rate: Option<bitcoin::FeeRate>,
}

/// Response to the `"blockchain.headers.subscribe"` method.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HeadersSubscribeResp {
    /// The latest block header known to the server.
    #[serde(
        rename = "hex",
        deserialize_with = "crate::custom_serde::from_consensus_hex",
        serialize_with = "crate::custom_serde::to_consensus_hex"
    )]
    pub header: bitcoin::block::Header,

    /// The height of the block in the header.
    pub height: u32,
}

/// Response to the `"blockchain.relayfee"` method.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct RelayFeeResp {
    /// The minimum fee amount that the server will accept for relaying transactions.
    #[serde(with = "bitcoin::amount::serde::as_btc")]
    pub fee: Amount,
}

/// Response to the `"blockchain.scripthash.get_balance"` method.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GetBalanceResp {
    /// The confirmed balance in satoshis.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub confirmed: Amount,

    /// The unconfirmed balance in satoshis.
    ///
    /// Can be negative when confirmed outputs are spent in the mempool.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub unconfirmed: SignedAmount,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ConfirmedTx {
    /// The transaction ID.
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,

    /// The height of the block containing this transaction.
    pub height: absolute::Height,
}

/// An unconfirmed transaction returned by `"blockchain.scripthash.get_mempool"`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MempoolTx {
    /// The transaction ID.
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,

    /// The fee paid by the transaction in satoshis.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub fee: bitcoin::Amount,

    /// Whether all inputs are confirmed.
    #[serde(
        rename = "height",
        deserialize_with = "crate::custom_serde::all_inputs_confirmed_bool_from_height",
        serialize_with = "crate::custom_serde::all_inputs_confirmed_bool_to_height"
    )]
    pub confirmed_inputs: bool,
}

/// Response entry from the `"blockchain.scripthash.listunspent"` method.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Utxo {
    /// The height of the block in which the UTXO was confirmed, or `0` if unconfirmed.
    pub height: absolute::Height,

    /// The output index of the transaction.
    pub tx_pos: usize,

    /// The transaction ID that created this UTXO.
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,

    /// The value of the UTXO in satoshis.
    #[serde(with = "bitcoin::amount::serde::as_sat")]
    pub value: bitcoin::Amount,
}

/// Response to the `"blockchain.transaction.get"` method.
///
/// Contains the full deserialized transaction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct FullTx {
    /// The full transaction.
    #[serde(
        deserialize_with = "crate::custom_serde::from_consensus_hex",
        serialize_with = "crate::custom_serde::to_consensus_hex"
    )]
    pub tx: bitcoin::Transaction,
}

/// Response to the `"blockchain.transaction.get_merkle"` method.
///
/// Contains a Merkle proof of inclusion in a block.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct TxidFromPos {
    /// The transaction ID located at the specified position.
    pub txid: bitcoin::Txid,
}

/// Response entry from the `"mempool.get_fee_histogram"` method.
///
/// Describes one fee-rate bin and the total weight of transactions at or above that rate.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct FeePair {
    /// The minimum fee rate (in sat/vB) for this bucket.
    #[serde(
        deserialize_with = "crate::custom_serde::feerate_from_sat_per_byte",
        serialize_with = "crate::custom_serde::feerate_to_sat_per_byte"
    )]
    pub fee_rate: bitcoin::FeeRate,

    /// The total weight (in vbytes) of transactions at or above this fee rate.
    #[serde(
        deserialize_with = "crate::custom_serde::weight_from_vb",
        serialize_with = "crate::custom_serde::weight_to_vb"
    )]
    pub weight: bitcoin::Weight,
}

/// Response to the `"blockchain.transaction.broadcast_package"` method (non-verbose mode).
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-transaction-broadcast-package>
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BroadcastPackageResp {
    /// Whether the package was accepted by the server.
    pub success: bool,

    /// Per-transaction errors for txs that were not accepted, if any.
    ///
    /// Present when `success` is `false`.
    pub errors: Option<Vec<BroadcastPackageError>>,
}

/// A per-transaction rejection inside [`BroadcastPackageResp::errors`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BroadcastPackageError {
    /// The rejected transaction's txid.
    pub txid: bitcoin::Txid,

    /// The rejection reason (e.g. `"bad-txns-inputs-missingorspent"`).
    pub error: String,
}

/// Response to the `"mempool.get_info"` method.
///
/// Provides fee-related information about the server's mempool.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#mempool-get-info>
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MempoolInfoResp {
    /// The minimum fee rate for a transaction to be accepted into the mempool.
    #[serde(
        deserialize_with = "crate::custom_serde::feerate_from_btc_per_kb",
        serialize_with = "crate::custom_serde::feerate_to_btc_per_kb"
    )]
    pub mempoolminfee: bitcoin::FeeRate,

    /// The minimum relay fee rate.
    #[serde(
        deserialize_with = "crate::custom_serde::feerate_from_btc_per_kb",
        serialize_with = "crate::custom_serde::feerate_to_btc_per_kb"
    )]
    pub minrelaytxfee: bitcoin::FeeRate,

    /// The incremental relay fee rate.
    #[serde(
        deserialize_with = "crate::custom_serde::feerate_from_btc_per_kb",
        serialize_with = "crate::custom_serde::feerate_to_btc_per_kb"
    )]
    pub incrementalrelayfee: bitcoin::FeeRate,
}

/// Response to the `"server.features"` method.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ServerHostValues {
    /// SSL Port.
    pub ssl_port: Option<u16>,
    /// TCP Port.
    pub tcp_port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_balance_preserves_negative_unconfirmed() {
        let response: GetBalanceResp =
            serde_json::from_str(r#"{"confirmed":0,"unconfirmed":-100}"#).unwrap();

        assert_eq!(response.unconfirmed, SignedAmount::from_sat(-100));
    }

    /// Serializes `value`, deserializes the result back, and checks that nothing was lost.
    ///
    /// This is what guarantees that a serialized response is still valid Electrum wire data.
    fn assert_round_trip<T>(value: T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + core::fmt::Debug + PartialEq,
    {
        let json = serde_json::to_value(&value).expect("must serialize");
        let got = serde_json::from_value::<T>(json.clone()).expect("must deserialize: {json}");
        assert_eq!(got, value, "round trip must be lossless: {json}");
    }

    fn header() -> bitcoin::block::Header {
        bitcoin::block::Header {
            version: bitcoin::block::Version::ONE,
            prev_blockhash: BlockHash::all_zeros(),
            merkle_root: bitcoin::TxMerkleNode::all_zeros(),
            time: 1_231_006_505,
            bits: bitcoin::CompactTarget::from_consensus(0x1d00_ffff),
            nonce: 2_083_236_893,
        }
    }

    fn txid() -> bitcoin::Txid {
        "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b"
            .parse()
            .expect("must parse")
    }

    fn height(h: u32) -> absolute::Height {
        absolute::Height::from_consensus(h).expect("must be a valid height")
    }

    #[test]
    fn round_trip_responses() {
        assert_round_trip(HeaderResp { header: header() });
        assert_round_trip(HeaderWithProofResp {
            branch: vec![DoubleSHA::hash(b"branch")],
            header: header(),
            root: DoubleSHA::hash(b"root"),
        });
        assert_round_trip(HeadersResp {
            count: 2,
            headers: vec![header(), header()],
            max: 2016,
        });
        assert_round_trip(HeadersWithCheckpointResp {
            count: 1,
            headers: vec![header()],
            max: 2016,
            root: DoubleSHA::hash(b"root"),
            branch: vec![DoubleSHA::hash(b"branch")],
        });
        // 25_000 sat/kwu is 0.001 BTC/kvB, which is exactly representable as an `f32`.
        assert_round_trip(EstimateFeeResp {
            fee_rate: Some(bitcoin::FeeRate::from_sat_per_kwu(25_000)),
        });
        assert_round_trip(EstimateFeeResp { fee_rate: None });
        assert_round_trip(HeadersSubscribeResp {
            header: header(),
            height: 840_000,
        });
        assert_round_trip(RelayFeeResp {
            fee: Amount::from_sat(1_000),
        });
        assert_round_trip(GetBalanceResp {
            confirmed: Amount::from_sat(123_456),
            unconfirmed: SignedAmount::from_sat(-500),
        });
        assert_round_trip(ConfirmedTx {
            txid: txid(),
            height: height(840_000),
        });
        assert_round_trip(MempoolTx {
            txid: txid(),
            fee: Amount::from_sat(1_500),
            confirmed_inputs: true,
        });
        assert_round_trip(Utxo {
            height: height(840_000),
            tx_pos: 1,
            txid: txid(),
            value: Amount::from_sat(50_000),
        });
        assert_round_trip(FullTx {
            tx: bitcoin::Transaction {
                version: bitcoin::transaction::Version::TWO,
                lock_time: absolute::LockTime::ZERO,
                input: vec![],
                output: vec![],
            },
        });
        assert_round_trip(TxMerkle {
            block_height: height(840_000),
            merkle: vec![DoubleSHA::hash(b"merkle")],
            pos: 3,
        });
        assert_round_trip(TxidFromPos { txid: txid() });
        assert_round_trip(ServerVersionResp {
            server_software: "ElectrumX 1.18.0".to_string(),
            protocol_version: "1.6".to_string(),
        });
        assert_round_trip(BroadcastPackageResp {
            success: false,
            errors: Some(vec![BroadcastPackageError {
                txid: txid(),
                error: "bad-txns-inputs-missingorspent".to_string(),
            }]),
        });
        assert_round_trip(MempoolInfoResp {
            mempoolminfee: bitcoin::FeeRate::from_sat_per_kwu(25_000),
            minrelaytxfee: bitcoin::FeeRate::from_sat_per_kwu(25_000),
            incrementalrelayfee: bitcoin::FeeRate::from_sat_per_kwu(25_000),
        });
        // 250 sat/kwu is 1 sat/vB and 4000 WU is 1000 vB, so neither conversion loses precision.
        assert_round_trip(FeePair {
            fee_rate: bitcoin::FeeRate::from_sat_per_kwu(250),
            weight: bitcoin::Weight::from_vb(1_000).expect("must not overflow"),
        });
        assert_round_trip(ServerHostValues {
            ssl_port: Some(50002),
            tcp_port: None,
        });
        assert_round_trip(ServerFeatures {
            hosts: [(
                "electrum.example.com".to_string(),
                ServerHostValues {
                    ssl_port: Some(50002),
                    tcp_port: Some(50001),
                },
            )]
            .into_iter()
            .collect(),
            genesis_hash: "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
                .parse()
                .expect("must parse"),
            hash_function: "sha256".to_string(),
            server_version: "ElectrumX 1.16.0".to_string(),
            protocol_max: "1.4".to_string(),
            protocol_min: "1.4".to_string(),
            pruning: None,
        });
    }

    /// `Tx` is untagged, so a round trip must land back on the same variant.
    #[test]
    fn round_trip_tx_preserves_variant() {
        for tx in [
            Tx::Mempool(MempoolTx {
                txid: txid(),
                fee: Amount::from_sat(1_500),
                confirmed_inputs: true,
            }),
            Tx::Mempool(MempoolTx {
                txid: txid(),
                fee: Amount::from_sat(1_500),
                confirmed_inputs: false,
            }),
            Tx::Confirmed(ConfirmedTx {
                txid: txid(),
                height: height(840_000),
            }),
        ] {
            assert_round_trip(tx);
        }
    }

    /// `MempoolTx::confirmed_inputs` must go back out as the Electrum `height` field.
    #[test]
    fn mempool_tx_serializes_height_not_bool() {
        let tx = MempoolTx {
            txid: txid(),
            fee: Amount::from_sat(1_500),
            confirmed_inputs: true,
        };
        assert_eq!(
            serde_json::to_value(&tx).expect("must serialize"),
            serde_json::json!({
                "tx_hash": "4a5e1e4baab89f3a32518a88c31bc87f618f76673e2cc77ab2127b7afdeda33b",
                "fee": 1_500,
                "height": 0,
            }),
        );
        assert_eq!(
            serde_json::to_value(MempoolTx {
                confirmed_inputs: false,
                ..tx
            })
            .expect("must serialize")["height"],
            serde_json::json!(-1),
        );
    }
}
