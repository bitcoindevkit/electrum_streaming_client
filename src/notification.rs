use serde::Deserialize;

use crate::{response, ElectrumScriptHash, ElectrumScriptStatus, RawNotification};

#[derive(Debug, Clone)]
pub enum Notification {
    Header(HeaderNotification),
    ScriptHash(ScriptHashNotification),
    Unknown(UnknownNotification),
}

impl Notification {
    pub fn new(raw: &RawNotification) -> Result<Self, serde_json::Error> {
        let RawNotification { method, params, .. } = raw;
        match method.as_ref() {
            "blockchain.headers.subscribe" => {
                HeaderNotification::deserialize(params).map(Notification::Header)
            }
            "blockchain.scripthash.subscribe" => {
                ScriptHashNotification::deserialize(params).map(Notification::ScriptHash)
            }
            _ => Ok(Notification::Unknown(raw.clone())),
        }
    }
}

pub type UnknownNotification = RawNotification;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeaderNotification {
    param_0: response::HeadersSubscribeResp,
}

impl HeaderNotification {
    pub fn height(&self) -> u32 {
        self.param_0.height
    }

    pub fn header(&self) -> &bitcoin::block::Header {
        &self.param_0.header
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScriptHashNotification {
    param_0: ElectrumScriptHash,
    param_1: ElectrumScriptStatus,
}

impl ScriptHashNotification {
    pub fn script_hash(&self) -> ElectrumScriptHash {
        self.param_0
    }

    pub fn script_status(&self) -> ElectrumScriptStatus {
        self.param_1
    }
}
