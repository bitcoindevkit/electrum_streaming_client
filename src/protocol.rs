// SPDX-License-Identifier: MIT OR Apache-2.0

//! Low-level JSON-RPC protocol types for the Electrum wire format.
//!
//! This module defines the raw message types exchanged between client and server,
//! including requests, responses, notifications, and error wrappers.

use serde_json::Value;
use std::fmt::Display;

use crate::{CowStr, Request};

/// The JSON-RPC protocol version supported by this client.
///
/// Always set to `"2.0"` per the Electrum protocol specification.
pub const JSONRPC_VERSION_2_0: &str = "2.0";

/// Represents the `jsonrpc` version field in JSON-RPC messages.
///
/// In Electrum, this is always the string `"2.0"`, as required by the JSON-RPC 2.0 specification.
/// It appears in all standard requests, responses, and notifications.
///
/// This type ensures consistent serialization and deserialization of the version field.
#[derive(Debug, Clone, Copy)]
pub struct Version;

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(JSONRPC_VERSION_2_0)
    }
}

impl AsRef<str> for Version {
    fn as_ref(&self) -> &str {
        JSONRPC_VERSION_2_0
    }
}

/// A raw server-initiated JSON-RPC notification.
///
/// These are Electrum messages that have a `"method"` and `"params"`, but no `"id"` field.
/// Typically emitted for subscriptions like `blockchain.headers.subscribe`.
#[derive(Debug, Clone, serde::Deserialize)]
#[allow(clippy::manual_non_exhaustive)]
pub struct RawNotification {
    /// The JSON-RPC protocol version (should always be `"2.0"`).
    #[serde(
        rename(deserialize = "jsonrpc"),
        deserialize_with = "crate::custom_serde::version"
    )]
    pub version: Version,

    /// The method name of the notification (e.g., `"blockchain.headers.subscribe"`).
    pub method: CowStr,

    /// The raw parameters associated with the notification.
    pub params: Value,
}

/// A raw JSON-RPC response from the Electrum server.
///
/// This is the server's response to a client-issued request. It may contain either a `result`
/// or an `error` (as per the JSON-RPC spec).
#[derive(Debug, Clone, serde::Deserialize)]
#[allow(clippy::manual_non_exhaustive)]
pub struct RawResponse {
    /// The JSON-RPC protocol version (should always be `"2.0"`).
    #[serde(
        rename(deserialize = "jsonrpc"),
        deserialize_with = "crate::custom_serde::version"
    )]
    pub version: Version,

    /// The ID that matches the request this response is answering.
    pub id: u32,

    /// The result if the request succeeded, or the error object if it failed.
    #[serde(flatten, deserialize_with = "crate::custom_serde::result")]
    pub result: Result<Value, Value>,
}

/// A raw incoming message from the Electrum server.
///
/// This type represents either a JSON-RPC notification (e.g., for a subscription)
/// or a response to a previously issued request.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum RawIncoming {
    /// A server-initiated notification (e.g., from a subscription).
    Notification(RawNotification),

    /// A response to a previously sent request.
    Response(RawResponse),
}

/// A raw JSON-RPC request to be sent to the Electrum server.
///
/// This struct is constructed before serialization and sending. It includes all required
/// JSON-RPC fields for method calls.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RawRequest {
    /// The JSON-RPC version string (usually `"2.0"`).
    pub jsonrpc: CowStr,

    /// The client-assigned request ID (used to correlate with responses).
    pub id: u32,

    /// The method to be invoked (e.g., `"blockchain.headers.subscribe"`).
    pub method: CowStr,

    /// The parameters passed to the method.
    pub params: Vec<Value>,
}

impl RawRequest {
    /// Constructs a new JSON-RPC request with the given ID, method, and parameters.
    ///
    /// This sets the JSON-RPC version to `"2.0"`.
    pub fn new(id: u32, method: CowStr, params: Vec<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION_2_0.into(),
            id,
            method,
            params,
        }
    }

    pub fn from_request<Req: Request>(id: u32, req: Req) -> Self {
        (id, req).into()
    }
}

/// Represents either a single item or a batch of items.
///
/// This enum is used to generalize over sending one or many requests in the same operation. I.e.
/// to the Electrum server.
///
/// Use `From` implementations to easily convert from `T` or `Vec<T>`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RawOneOrMany<T> {
    Single(T),
    Batch(Vec<T>),
}

impl<T> RawOneOrMany<T> {
    /// Converts this `RawOneOrMany` into a `Vec<T>`.
    ///
    /// If it is a `Single`, returns a one-element vector. If it is a `Batch`, returns the inner vector.
    pub fn into_vec(self) -> Vec<T> {
        match self {
            RawOneOrMany::Single(item) => vec![item],
            RawOneOrMany::Batch(batch) => batch,
        }
    }

    /// Pushes a new item into the given `Option<RawOneOrMany<T>>`, creating or extending the batch.
    ///
    /// If the option is `None`, it becomes `Some(Single(item))`. If it already contains a value,
    /// it is converted into a `Batch` and the item is appended.
    pub fn push_opt(opt: &mut Option<Self>, item: T) {
        *opt = match opt.take() {
            None => Some(Self::Single(item)),
            Some(maybe_batch) => {
                let mut items = maybe_batch.into_vec();
                items.push(item);
                Some(RawOneOrMany::Batch(items))
            }
        }
    }

    pub fn map<T2>(self, f: impl Fn(T) -> T2) -> RawOneOrMany<T2> {
        match self {
            RawOneOrMany::Single(t) => RawOneOrMany::Single(f(t)),
            RawOneOrMany::Batch(items) => RawOneOrMany::Batch(items.into_iter().map(f).collect()),
        }
    }

    pub fn map_into<T2>(self) -> RawOneOrMany<T2>
    where
        T: Into<T2>,
    {
        self.map(Into::into)
    }
}

impl<T> From<T> for RawOneOrMany<T> {
    fn from(value: T) -> Self {
        Self::Single(value)
    }
}

impl<T> From<Vec<T>> for RawOneOrMany<T> {
    fn from(value: Vec<T>) -> Self {
        Self::Batch(value)
    }
}

/// Electrum server responds with an error.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ResponseError(pub(crate) Value);

impl std::fmt::Display for ResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Response.error: {}", self.0)
    }
}

impl std::error::Error for ResponseError {}
