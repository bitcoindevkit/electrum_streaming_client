use bitcoin::{
    absolute,
    hashes::{Hash, HashEngine},
    Amount,
};

use crate::DoubleSHA;

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct HeaderResp {
    #[serde(deserialize_with = "crate::custom_serde::from_consensus_hex")]
    pub header: bitcoin::block::Header,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct HeaderWithProofResp {
    pub branch: Vec<DoubleSHA>,
    #[serde(deserialize_with = "crate::custom_serde::from_consensus_hex")]
    pub header: bitcoin::block::Header,
    pub root: DoubleSHA,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeadersResp {
    pub count: usize,
    #[serde(
        rename = "hex",
        deserialize_with = "crate::custom_serde::from_cancat_consensus_hex"
    )]
    pub headers: Vec<bitcoin::block::Header>,
    pub max: usize,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeadersWithCheckpointResp {
    pub count: usize,
    #[serde(
        rename = "hex",
        deserialize_with = "crate::custom_serde::from_cancat_consensus_hex"
    )]
    pub headers: Vec<bitcoin::block::Header>,
    pub max: usize,
    pub root: DoubleSHA,
    pub branch: Vec<DoubleSHA>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct EstimateFeeResp {
    #[serde(deserialize_with = "crate::custom_serde::feerate_opt_from_btc_per_kb")]
    pub fee_rate: Option<bitcoin::FeeRate>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeadersSubscribeResp {
    #[serde(
        rename = "hex",
        deserialize_with = "crate::custom_serde::from_consensus_hex"
    )]
    pub header: bitcoin::block::Header,
    pub height: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct RelayFeeResp {
    #[serde(deserialize_with = "crate::custom_serde::amount_from_btc")]
    pub fee: Amount,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GetBalanceResp {
    #[serde(deserialize_with = "crate::custom_serde::amount_from_sats")]
    pub confirmed: Amount,
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
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConfirmedTx {
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,
    pub height: absolute::Height,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MempoolTx {
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,
    #[serde(deserialize_with = "crate::custom_serde::amount_from_sats")]
    pub fee: bitcoin::Amount,
    #[serde(
        rename = "height",
        deserialize_with = "crate::custom_serde::all_inputs_confirmed_bool_from_height"
    )]
    pub all_inputs_confirmed: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Utxo {
    pub height: absolute::Height,
    pub tx_pos: usize,
    #[serde(rename = "tx_hash")]
    pub txid: bitcoin::Txid,
    #[serde(deserialize_with = "crate::custom_serde::amount_from_sats")]
    pub value: bitcoin::Amount,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct FullTx {
    #[serde(deserialize_with = "crate::custom_serde::from_consensus_hex")]
    pub tx: bitcoin::Transaction,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TxMerkle {
    pub block_height: absolute::Height,
    pub merkle: Vec<DoubleSHA>,
    pub pos: usize,
}

impl TxMerkle {
    /// Returns the merkle root of a [`Header`] which satisfies this proof.
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

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(transparent)]
pub struct TxidFromPos {
    pub txid: bitcoin::Txid,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FeePair {
    #[serde(deserialize_with = "crate::custom_serde::feerate_from_sat_per_byte")]
    pub fee_rate: bitcoin::FeeRate,
    #[serde(deserialize_with = "crate::custom_serde::weight_from_vb")]
    pub weight: bitcoin::Weight,
}
