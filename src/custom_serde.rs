use bitcoin::{
    consensus::{deserialize_partial, encode::deserialize_hex},
    hex::FromHex,
};
use serde::{
    de::{Error, Unexpected},
    Deserialize, Deserializer,
};
use serde_json::Value;

use crate::{CowStr, Version, JSONRPC_VERSION_2_0};

pub fn from_consensus_hex<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: bitcoin::consensus::encode::Decodable,
    D: Deserializer<'de>,
{
    let hex_str = String::deserialize(deserializer)?;
    deserialize_hex(&hex_str).map_err(serde::de::Error::custom)
}

pub fn from_cancat_consensus_hex<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: bitcoin::consensus::encode::Decodable,
    D: Deserializer<'de>,
{
    let hex_str = String::deserialize(deserializer)?;
    let data = Vec::<u8>::from_hex(&hex_str).map_err(serde::de::Error::custom)?;

    let mut items = Vec::<T>::new();
    let mut read_start = 0_usize;
    while read_start < data.len() {
        let (item, read_count) =
            deserialize_partial::<T>(&data[read_start..]).map_err(serde::de::Error::custom)?;
        read_start += read_count;
        items.push(item);
    }
    Ok(items)
}

pub fn feerate_opt_from_btc_per_kb<'de, D>(
    deserializer: D,
) -> Result<Option<bitcoin::FeeRate>, D::Error>
where
    D: Deserializer<'de>,
{
    let btc_per_kvb = f32::deserialize(deserializer)?;
    if btc_per_kvb.is_sign_negative() {
        return Ok(None);
    }
    let sat_per_kwu = btc_per_kvb * (100_000_000.0 / 4.0);
    Ok(Some(bitcoin::FeeRate::from_sat_per_kwu(sat_per_kwu as _)))
}

pub fn feerate_from_sat_per_byte<'de, D>(deserializer: D) -> Result<bitcoin::FeeRate, D::Error>
where
    D: Deserializer<'de>,
{
    let sat_per_vb = f32::deserialize(deserializer)?;
    let sat_per_kwu = sat_per_vb * (1000.0 / 4.0);
    Ok(bitcoin::FeeRate::from_sat_per_kwu(sat_per_kwu as _))
}

pub fn weight_from_vb<'de, D>(deserializer: D) -> Result<bitcoin::Weight, D::Error>
where
    D: Deserializer<'de>,
{
    let vb = u64::deserialize(deserializer)?;
    let weight = bitcoin::Weight::from_vb(vb).ok_or(serde::de::Error::custom(
        serde_json::Error::custom("overflow: vb value cannot fit in to WU"),
    ))?;
    Ok(weight)
}

pub fn amount_from_btc<'de, D>(deserializer: D) -> Result<bitcoin::Amount, D::Error>
where
    D: Deserializer<'de>,
{
    let btc = f64::deserialize(deserializer)?;
    bitcoin::Amount::from_btc(btc).map_err(serde::de::Error::custom)
}

pub fn amount_from_sats<'de, D>(deserializer: D) -> Result<bitcoin::Amount, D::Error>
where
    D: Deserializer<'de>,
{
    let sats = u64::deserialize(deserializer)?;
    Ok(bitcoin::Amount::from_sat(sats))
}

pub fn amount_from_maybe_negative_sats<'de, D>(deserializer: D) -> Result<bitcoin::Amount, D::Error>
where
    D: Deserializer<'de>,
{
    let sats = i64::deserialize(deserializer)?.unsigned_abs();
    Ok(bitcoin::Amount::from_sat(sats))
}

pub fn all_inputs_confirmed_bool_from_height<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    match i64::deserialize(deserializer)? {
        0 => Ok(true),
        -1 => Ok(false),
        unexpected => Err(serde::de::Error::custom(serde_json::Error::invalid_value(
            Unexpected::Signed(unexpected),
            &"0 or -1",
        ))),
    }
}

pub fn result<'de, D>(deserializer: D) -> Result<Result<Value, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, Deserialize)]
    pub enum T {
        #[serde(rename(deserialize = "result"))]
        Result(Value),
        #[serde(rename(deserialize = "error"))]
        Error(Value),
    }
    T::deserialize(deserializer).map(|r| match r {
        T::Result(v) => Ok(v),
        T::Error(e) => Err(e),
    })
}

pub fn version<'de, D>(deserializer: D) -> Result<Version, D::Error>
where
    D: Deserializer<'de>,
{
    let version_str = CowStr::deserialize(deserializer)?;
    if version_str != JSONRPC_VERSION_2_0 {
        return Err(serde::de::Error::custom("JSON-RPC version is not 2.0"));
    }
    Ok(Version)
}
