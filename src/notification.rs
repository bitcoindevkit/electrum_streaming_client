//! Defines parsed Electrum server notifications.
//!
//! This module provides the [`Notification`] enum, which categorizes incoming Electrum
//! notifications based on their method type. Currently supported variants include:
//!
//! - [`Notification::Header`] for `"blockchain.headers.subscribe"`
//! - [`Notification::ScriptHash`] for `"blockchain.scripthash.subscribe"`
//! - [`Notification::Unknown`] for unrecognized or unsupported methods
//!
//! Each variant wraps a struct that contains the deserialized payload for that notification type.
//! Use [`Notification::new`] to construct a typed [`Notification`] from a raw JSON-RPC notification.
//!
//! This is useful for higher-level consumers who want to match against known server-side events.

use serde::Deserialize;

use crate::{response, ElectrumScriptHash, ElectrumScriptStatus, RawNotification};

/// A parsed Electrum server notification.
///
/// This enum represents server-initiated messages received outside the context of a request.
/// Use [`Notification::new`] to convert a raw JSON-RPC notification into a typed variant.
///
/// Known notification types are parsed into structured variants. Unknown or unsupported types are
/// preserved as-is in [`Notification::Unknown`].
#[derive(Debug, Clone)]
pub enum Notification {
    /// A notification from `"blockchain.headers.subscribe"` indicating a new best block header.
    Header(HeaderNotification),

    /// A notification from `"blockchain.scripthash.subscribe"` indicating a change in script
    /// status.
    ScriptHash(ScriptHashNotification),

    /// A catch-all for notifications with unrecognized methods.
    ///
    /// The original [`RawNotification`] is preserved for downstream inspection.
    Unknown(UnknownNotification),
}

impl Notification {
    /// Attempts to parse a [`RawNotification`] into a typed [`Notification`] variant.
    ///
    /// Returns `Ok` with a known variant if the method is recognized, or [`Notification::Unknown`]
    /// otherwise.
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

/// A type alias for unrecognized Electrum notifications.
///
/// Used when the method name is not handled explicitly by the client. The raw JSON-RPC
/// notification is preserved without interpretation.
pub type UnknownNotification = RawNotification;

/// A notification indicating the current best block header on the chain tip.
///
/// Corresponds to the `"blockchain.headers.subscribe"` Electrum notification method.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeaderNotification {
    param_0: response::HeadersSubscribeResp,
}

impl HeaderNotification {
    /// Returns the height of the new best block.
    pub fn height(&self) -> u32 {
        self.param_0.height
    }

    /// Returns a reference to the new best block header.
    pub fn header(&self) -> &bitcoin::block::Header {
        &self.param_0.header
    }
}
/// A notification indicating a change in the status of a specific script hash.
///
/// Corresponds to the `"blockchain.scripthash.subscribe"` Electrum notification method.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScriptHashNotification {
    param_0: ElectrumScriptHash,
    param_1: ElectrumScriptStatus,
}

impl ScriptHashNotification {
    /// Returns the script hash associated with the notification.
    pub fn script_hash(&self) -> ElectrumScriptHash {
        self.param_0
    }

    /// Returns the new script status associated with the script hash.
    pub fn script_status(&self) -> ElectrumScriptStatus {
        self.param_1
    }
}
