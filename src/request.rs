use bitcoin::{consensus::Encodable, hex::DisplayHex, Script, Txid};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    response, CowStr, ElectrumScriptHash, ElectrumScriptStatus, MethodAndParams, ResponseError,
};

/// The caller-facing [`Request`] data type.
pub trait Request: Clone {
    /// The associated response type of this request.
    type Response: for<'a> Deserialize<'a> + Clone + Send + Sync + 'static;

    fn to_method_and_params(&self) -> MethodAndParams;
}

#[derive(Debug, Clone)]
pub struct Custom {
    pub method: CowStr,
    pub params: Vec<Value>,
}

impl Request for Custom {
    type Response = serde_json::Value;

    fn to_method_and_params(&self) -> MethodAndParams {
        (self.method.clone(), self.params.clone())
    }
}

/// Occurs when a request fails.
#[derive(Debug)]
pub enum Error<DispatchError> {
    Dispatch(DispatchError),
    Canceled,
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

#[derive(Debug, Clone)]
pub struct Header {
    pub height: u32,
}

impl Request for Header {
    type Response = response::HeaderResp;
    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.block.header".into(), vec![self.height.into()])
    }
}

#[derive(Debug, Clone)]
pub struct HeaderWithProof {
    pub height: u32,
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

#[derive(Debug, Clone)]
pub struct Headers {
    pub start_height: u32,
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

#[derive(Debug, Clone)]
pub struct HeadersWithCheckpoint {
    pub start_height: u32,
    pub count: usize,
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

#[derive(Debug, Clone)]
pub struct HeadersSubscribe;

impl Request for HeadersSubscribe {
    type Response = response::HeadersSubscribeResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.headers.subscribe".into(), vec![])
    }
}

#[derive(Debug, Clone)]
pub struct RelayFee;

impl Request for RelayFee {
    type Response = response::RelayFeeResp;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("blockchain.relayfee".into(), vec![])
    }
}

#[derive(Debug, Clone)]
pub struct GetBalance {
    pub script_hash: ElectrumScriptHash,
}

impl GetBalance {
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

#[derive(Debug, Clone)]
pub struct GetHistory {
    pub script_hash: ElectrumScriptHash,
}

impl GetHistory {
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

#[derive(Debug, Clone)]
pub struct ListUnspent {
    pub script_hash: ElectrumScriptHash,
}

impl ListUnspent {
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

#[derive(Debug, Clone)]
pub struct ScriptHashSubscribe {
    pub script_hash: ElectrumScriptHash,
}

impl ScriptHashSubscribe {
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
pub struct ScriptHashUnsubscribe {
    pub script_hash: ElectrumScriptHash,
}

impl ScriptHashUnsubscribe {
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
pub struct GetTx(pub bitcoin::Txid);

impl Request for GetTx {
    type Response = response::FullTx;

    fn to_method_and_params(&self) -> MethodAndParams {
        (
            "blockchain.transaction.get".into(),
            vec![self.0.to_string().into()],
        )
    }
}

#[derive(Debug, Clone)]
pub struct GetTxMerkle {
    pub txid: Txid,
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

#[derive(Debug, Clone)]
pub struct GetTxidFromPos {
    pub height: u32,
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

#[derive(Debug, Clone)]
pub struct GetFeeHistogram;

impl Request for GetFeeHistogram {
    type Response = Vec<response::FeePair>;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("mempool.get_fee_histogram".into(), vec![])
    }
}

#[derive(Debug, Clone)]
pub struct Banner;

impl Request for Banner {
    type Response = String;

    fn to_method_and_params(&self) -> MethodAndParams {
        ("server.banner".into(), vec![])
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ping;

impl Request for Ping {
    type Response = ();

    fn to_method_and_params(&self) -> MethodAndParams {
        ("server.ping".into(), vec![])
    }
}
