//! Defines the request abstraction used to interact with the Electrum server.
//!
//! This module provides the [`Request`] trait, which describes a type-safe wrapper around an
//! Electrum JSON-RPC method and its parameters, along with the expected response type.
//!
//! Each request type implements [`Request`] by defining:
//! - the JSON-RPC method name and parameter list via [`to_method_and_params`].
//! - an associated [`Response`] type for deserialization.
//!
//! The module also includes a [`Custom`] request type for dynamically constructed method calls,
//! and the [`Error`] enum for representing failures in request dispatch or response handling.
//!
//! This abstraction allows request types to be encoded independently of any I/O mechanism,
//! making them suitable for use in a sans-io architecture.
//!
//! [`to_method_and_params`]: Request::to_method_and_params
//! [`Response`]: Request::Response

use bitcoin::{consensus::Encodable, hex::DisplayHex, Script, Txid};

use crate::{
    response, CowStr, ElectrumScriptHash, ElectrumScriptStatus, MethodAndParams, ResponseError,
};

/// A trait representing a typed Electrum JSON-RPC request.
///
/// Typically, each variant of an Electrum method is represented by a distinct type implementing
/// this trait.
pub trait Request: Clone {
    /// The expected response type for this request.
    ///
    /// This must be `Deserialize`, `Clone`, `Send`, and `'static` to allow usage across threads
    /// and in dynamic contexts.
    type Response: for<'a> serde::Deserialize<'a> + Clone + Send + Sync + 'static;

    /// Converts the request into its method name and parameter list.
    ///
    /// This is used to construct the raw JSON-RPC payload.
    fn to_method_and_params(&self) -> MethodAndParams;
}

/// A dynamically constructed request for arbitrary Electrum methods.
///
/// This type allows manual specification of the method name and parameters without needing a
/// strongly typed wrapper. It is useful for debugging, experimentation, or handling less common
/// server methods.
///
/// The response is returned as a generic `serde_json::Value`.
#[derive(Debug, Clone)]
pub struct Custom {
    /// The JSON-RPC method name to call.
    pub method: CowStr,

    /// The parameters to send with the method call.
    pub params: Vec<serde_json::Value>,
}

impl Request for Custom {
    type Response = serde_json::Value;

    fn to_method_and_params(&self) -> MethodAndParams {
        (self.method.clone(), self.params.clone())
    }
}

/// An error that occurred while dispatching or handling a request.
#[derive(Debug)]
pub enum Error<DispatchError> {
    /// The request failed to send or dispatch.
    ///
    /// This wraps a user-defined error type representing transport or queueing failures.
    Dispatch(DispatchError),

    /// The request was canceled before it could complete.
    ///
    /// This may happen if the request was dropped or explicitly aborted before a response arrived.
    Canceled,

    /// The server returned an error response for the request.
    ///
    /// This wraps a deserialized Electrum JSON-RPC error object.
    Response(ResponseError),
}

impl<SendError: std::fmt::Display> std::fmt::Display for Error<SendError> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dispatch(e) => write!(f, "Failed to dispatch request: {}", e),
            Self::Canceled => write!(f, "Request was canceled before being satisfied."),
            Self::Response(e) => write!(f, "Request satisfied with error: {}", e),
        }
    }
}

impl<SendError: std::error::Error> std::error::Error for Error<SendError> {}

/// A request for a block header at a specific height, without an inclusion proof.
///
/// This corresponds to the `"blockchain.block.header"` Electrum RPC method. It returns only the
/// serialized block header at the specified height.
///
/// If a Merkle proof to a checkpoint is desired—e.g., to verify inclusion relative to a known tip
/// without downloading intermediate headers—use [`HeaderWithProof`] instead.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-block-header>
#[derive(Debug, Clone)]
pub struct Header {
    /// The height of the block to fetch.
    pub height: u32,
}

impl Request for Header {
    type Response = response::HeaderResp;
    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.block.header".into(), vec![self.height.into()])
    }
}

/// A request for a block header along with a Merkle proof to a specified checkpoint.
///
/// This utilizes the `"blockchain.block.header"` Electrum RPC method with a non-zero `cp_height`
/// parameter. When `cp_height` is provided, the server returns:
///
/// - The block header at the specified `height`.
/// - A Merkle branch (`branch`) connecting that header to the root at `cp_height`.
/// - The Merkle root (`root`) of all headers up to and including `cp_height`.
///
/// This mechanism allows clients to verify the inclusion of a specific header in the blockchain
/// without downloading the entire header chain up to the checkpoint. It's particularly useful for
/// lightweight clients aiming to minimize bandwidth usage.
///
/// If no proof is required, consider using the [`Header`] type instead.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-block-header>
#[derive(Debug, Clone)]
pub struct HeaderWithProof {
    /// The height of the block whose header is being requested.
    pub height: u32,

    /// The checkpoint height used to generate the Merkle proof. Must be greater than or equal to `height`.
    pub cp_height: u32,
}

impl Request for HeaderWithProof {
    type Response = response::HeaderWithProofResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.block.header".into(),
            vec![self.height.into(), self.cp_height.into()],
        )
    }
}

/// A request for a sequence of block headers starting from a given height.
///
/// This corresponds to the `"blockchain.block.headers"` Electrum RPC method. It allows clients to
/// fetch a batch of headers, which is useful for syncing or verifying large sections of the chain.
///
/// Most Electrum servers impose a maximum `count` of 2016 headers per request (one difficulty
/// period).
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-block-headers>
#[derive(Debug, Clone)]
pub struct Headers {
    /// The height of the first block header to fetch.
    pub start_height: u32,

    /// The number of consecutive headers to retrieve.
    pub count: usize,
}

impl Request for Headers {
    type Response = response::HeadersResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.block.headers".into(), {
            vec![self.start_height.into(), self.count.into()]
        })
    }
}

/// A request for a sequence of block headers along with a Merkle inclusion proof to a checkpoint.
///
/// This corresponds to the `"blockchain.block.headers"` Electrum RPC method, with a `cp_height`
/// parameter. The server responds with a batch of headers, plus a Merkle proof connecting them to
/// a known checkpoint height.
///
/// This is useful for verifying multiple headers without downloading the full intermediate chain.
///
/// Most Electrum servers cap the maximum `count` at 2016 headers per request.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-block-headers>
#[derive(Debug, Clone)]
pub struct HeadersWithCheckpoint {
    /// The height of the first block header to fetch.
    pub start_height: u32,

    /// The number of consecutive headers to retrieve (typically capped at 2016).
    pub count: usize,

    /// The checkpoint height used to generate the inclusion proof.
    pub cp_height: u32,
}

impl Request for HeadersWithCheckpoint {
    type Response = response::HeadersWithCheckpointResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.block.headers".into(), {
            vec![
                self.start_height.into(),
                self.count.into(),
                self.cp_height.into(),
            ]
        })
    }
}

/// A request for an estimated fee rate needed to confirm a transaction within a target number of
/// blocks.
///
/// This corresponds to the `"blockchain.estimatefee"` Electrum RPC method. It returns the estimated
/// fee rate (in BTC per kilobyte) required to be included within the specified number of blocks.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-estimatefee>
#[derive(Debug, Clone)]
pub struct EstimateFee {
    /// The number of blocks to target for confirmation.
    pub number: usize,
}

impl Request for EstimateFee {
    type Response = response::EstimateFeeResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.estimatefee".into(), vec![self.number.into()])
    }
}

/// A subscription request for receiving notifications about new block headers.
///
/// This corresponds to the `"blockchain.headers.subscribe"` Electrum RPC method. Once subscribed,
/// the server will push a notification whenever a new block is added to the chain tip.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-headers-subscribe>
#[derive(Debug, Clone)]
pub struct HeadersSubscribe;

impl Request for HeadersSubscribe {
    type Response = response::HeadersSubscribeResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.headers.subscribe".into(), vec![])
    }
}

/// A request for the minimum fee rate accepted by the Electrum server's mempool.
///
/// This corresponds to the `"server.relayfee"` Electrum RPC method. It returns the minimum
/// fee rate (in BTC per kilobyte) that the server will accept for relaying transactions.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#server-relayfee>
#[derive(Debug, Clone)]
pub struct RelayFee;

impl Request for RelayFee {
    type Response = response::RelayFeeResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.relayfee".into(), vec![])
    }
}

/// A request for the confirmed and unconfirmed balance of a specific script hash.
///
/// This corresponds to the `"blockchain.scripthash.get_balance"` Electrum RPC method. It returns
/// both the confirmed balance (from mined transactions) and unconfirmed balance (from mempool
/// transactions) for the provided script hash.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-scripthash-get-balance>
#[derive(Debug, Clone)]
pub struct GetBalance {
    /// The script hash to query.
    pub script_hash: ElectrumScriptHash,
}

impl GetBalance {
    /// Constructs a `GetBalance` request from a Bitcoin script by hashing it to a script hash.
    ///
    /// This is a convenience method that transforms the provided script into the
    /// Electrum-compatible reversed script hash required by the server.
    pub fn from_script<S: AsRef<Script>>(script: S) -> Self {
        let script_hash = ElectrumScriptHash::new(script.as_ref());
        Self { script_hash }
    }
}

impl Request for GetBalance {
    type Response = response::GetBalanceResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.scripthash.get_balance".into(),
            vec![self.script_hash.to_string().into()],
        )
    }
}

/// A request for the transaction history of a specific script hash.
///
/// This corresponds to the `"blockchain.scripthash.get_history"` Electrum RPC method. It returns a
/// list of confirmed transactions (and their heights) that affect the specified script hash. It
/// does not include unconfirmed transactions.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-scripthash-get-history>
#[derive(Debug, Clone)]
pub struct GetHistory {
    /// The script hash whose history should be fetched.
    pub script_hash: ElectrumScriptHash,
}

impl GetHistory {
    /// Constructs a `GetHistory` request from a Bitcoin script by hashing it to a script hash.
    ///
    /// This is a convenience method that transforms the provided script into the
    /// Electrum-compatible reversed script hash required by the server.
    pub fn from_script<S: AsRef<Script>>(script: S) -> Self {
        let script_hash = ElectrumScriptHash::new(script.as_ref());
        Self { script_hash }
    }
}

impl Request for GetHistory {
    type Response = Vec<response::Tx>;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.scripthash.get_history".into(),
            vec![self.script_hash.to_string().into()],
        )
    }
}

/// Resource request for `blockchain.scripthash.get_mempool`.
///
/// Note that `electrs` does not support this endpoint.
#[derive(Debug, Clone)]
pub struct GetMempool {
    pub script_hash: ElectrumScriptHash,
}

impl GetMempool {
    /// Constructs a `GetMempool` request from a Bitcoin script by hashing it to a script hash.
    ///
    /// This helper simplifies creating a mempool query for the given script by converting it into
    /// the Electrum-compatible reversed script hash.
    pub fn from_script<S: AsRef<Script>>(script: S) -> Self {
        let script_hash = ElectrumScriptHash::new(script.as_ref());
        Self { script_hash }
    }
}

impl Request for GetMempool {
    // TODO: Dedicated type.
    type Response = Vec<response::MempoolTx>;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.scripthash.get_mempool".into(),
            vec![self.script_hash.to_string().into()],
        )
    }
}

/// A request for the list of unspent outputs associated with a script hash.
///
/// This corresponds to the `"blockchain.scripthash.listunspent"` Electrum RPC method. It returns
/// all UTXOs (unspent transaction outputs) controlled by the specified script hash, including their
/// value, height, and outpoint.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-scripthash-listunspent>
#[derive(Debug, Clone)]
pub struct ListUnspent {
    /// The script hash to query.
    pub script_hash: ElectrumScriptHash,
}

impl ListUnspent {
    /// Constructs a `ListUnspent` request from a Bitcoin script by hashing it to a script hash.
    ///
    /// This helper converts the script into the Electrum-style reversed SHA256 script hash used to
    /// identify addresses and outputs.
    pub fn from_script<S: AsRef<Script>>(script: S) -> Self {
        let script_hash = ElectrumScriptHash::new(script.as_ref());
        Self { script_hash }
    }
}

impl Request for ListUnspent {
    type Response = Vec<response::Utxo>;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.scripthash.listunspent".into(),
            vec![self.script_hash.to_string().into()],
        )
    }
}

/// A subscription request for receiving status updates on a script hash.
///
/// This corresponds to the `"blockchain.scripthash.subscribe"` Electrum RPC method. Once subscribed,
/// the server will notify the client whenever the status of the script hash changes—typically when
/// a new transaction is confirmed or enters the mempool.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-scripthash-subscribe>
#[derive(Debug, Clone)]
pub struct ScriptHashSubscribe {
    /// The script hash to subscribe to.
    pub script_hash: ElectrumScriptHash,
}

impl ScriptHashSubscribe {
    /// Constructs a `ScriptHashSubscribe` request from a Bitcoin script by hashing it to a script
    /// hash.
    ///
    /// This is a convenience method for subscribing to script activity without manually computing
    /// the Electrum-style reversed SHA256 script hash.
    pub fn from_script<S: AsRef<Script>>(script: S) -> Self {
        let script_hash = ElectrumScriptHash::new(script.as_ref());
        Self { script_hash }
    }
}

impl Request for ScriptHashSubscribe {
    type Response = Option<ElectrumScriptStatus>;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.scripthash.subscribe".into(),
            vec![self.script_hash.to_string().into()],
        )
    }
}

#[derive(Debug, Clone)]
/// A request to cancel a previous subscription to a script hash.
///
/// This corresponds to the `"blockchain.scripthash.unsubscribe"` Electrum RPC method. It tells the
/// server to stop sending notifications related to the specified script hash.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-scripthash-unsubscribe>
pub struct ScriptHashUnsubscribe {
    /// The script hash to unsubscribe from.
    pub script_hash: ElectrumScriptHash,
}

impl ScriptHashUnsubscribe {
    /// Constructs a `ScriptHashUnsubscribe` request from a Bitcoin script by hashing it to a script
    /// hash.
    ///
    /// This is a convenience method for unsubscribing without manually computing the script hash
    /// expected by the Electrum server.
    pub fn from_script<S: AsRef<Script>>(script: S) -> Self {
        let script_hash = ElectrumScriptHash::new(script.as_ref());
        Self { script_hash }
    }
}

impl Request for ScriptHashUnsubscribe {
    type Response = bool;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.scripthash.unsubscribe".into(),
            vec![self.script_hash.to_string().into()],
        )
    }
}

/// A request to broadcast a raw Bitcoin transaction to the network.
///
/// This corresponds to the `"blockchain.transaction.broadcast"` Electrum RPC method, which submits
/// the given transaction to the Electrum server's mempool.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-transaction-broadcast>
#[derive(Debug, Clone)]
pub struct BroadcastTx(pub bitcoin::Transaction);

impl Request for BroadcastTx {
    type Response = bitcoin::Txid;

    fn to_method_and_params(&self) -> MethodAndParams {
        let mut tx_bytes = Vec::<u8>::new();
        self.0.consensus_encode(&mut tx_bytes).expect("must encode");
        (
            "blockchain.transaction.broadcast".into(),
            vec![tx_bytes.to_lower_hex_string().into()],
        )
    }
}

#[derive(Debug, Clone)]
/// A request for the raw transaction corresponding to a given transaction ID.
///
/// This corresponds to the `"blockchain.transaction.get"` Electrum RPC method. It returns the full
/// transaction as a serialized hex string, typically used to inspect, rebroadcast, or verify it.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-transaction-get>
pub struct GetTx {
    /// The transaction ID to fetch.
    pub txid: Txid,
}

impl Request for GetTx {
    type Response = response::FullTx;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.transaction.get".into(),
            vec![self.txid.to_string().into()],
        )
    }
}

/// A request for the Merkle proof of a transaction's inclusion in a specific block.
///
/// This corresponds to the `"blockchain.transaction.get_merkle"` Electrum RPC method. It returns
/// the Merkle branch proving that the transaction is included in the block at the specified height.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-transaction-get-merkle>
#[derive(Debug, Clone)]
pub struct GetTxMerkle {
    /// The transaction ID to verify.
    pub txid: Txid,

    /// The height of the block that is claimed to contain the transaction.
    pub height: u32,
}

impl Request for GetTxMerkle {
    type Response = response::TxMerkle;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.transaction.get_merkle".into(),
            vec![self.txid.to_string().into(), self.height.into()],
        )
    }
}

/// A request to retrieve a transaction ID from a block position.
///
/// This corresponds to the `"blockchain.transaction.id_from_pos"` Electrum RPC method. It returns
/// the transaction ID at a given position within a block at the specified height.
///
/// This can be used for enumerating all transactions in a block by querying sequential positions.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#blockchain-transaction-id-from-pos>
#[derive(Debug, Clone)]
pub struct GetTxidFromPos {
    /// The height of the block containing the transaction.
    pub height: u32,

    /// The zero-based position of the transaction within the block.
    pub tx_pos: usize,
}

impl Request for GetTxidFromPos {
    type Response = response::TxidFromPos;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.transaction.id_from_pos".into(),
            vec![self.height.into(), self.tx_pos.into()],
        )
    }
}

/// A request for the current mempool fee histogram.
///
/// This corresponds to the `"mempool.get_fee_histogram"` Electrum RPC method. It returns a compact
/// histogram of fee rates (in sat/vB) and the total size of transactions at or above each rate,
/// allowing clients to estimate the mempool's fee landscape.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#mempool-get-fee-histogram>
#[derive(Debug, Clone)]
pub struct GetFeeHistogram;

impl Request for GetFeeHistogram {
    type Response = Vec<response::FeePair>;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("mempool.get_fee_histogram".into(), vec![])
    }
}

/// A request for the Electrum server's banner message.
///
/// This corresponds to the `"server.banner"` Electrum RPC method, which returns a server-defined
/// banner string, often used to display terms of service or notices to the user.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#server-banner>
#[derive(Debug, Clone)]
pub struct Banner;

impl Request for Banner {
    type Response = String;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("server.banner".into(), vec![])
    }
}

/// A ping request to verify the connection to the Electrum server.
///
/// This corresponds to the `"server.ping"` Electrum RPC method. It has no parameters and returns
/// `null`. It's used to keep the connection alive or measure basic liveness.
///
/// See: <https://electrum-protocol.readthedocs.io/en/latest/protocol-methods.html#server-ping>
#[derive(Debug, Clone, Copy)]
pub struct Ping;

impl Request for Ping {
    type Response = ();

    fn to_method_and_params(&self) -> MethodAndParams {
        ("server.ping".into(), vec![])
    }
}
