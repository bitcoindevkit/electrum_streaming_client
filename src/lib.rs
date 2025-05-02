mod batch_request;
mod client;
pub use client::*;
mod custom_serde;
mod hash_types;
pub mod io;
pub mod notification;
mod pending_request;
pub mod request;
pub mod response;
mod state;
pub use batch_request::*;
pub use hash_types::*;
pub use pending_request::*;
pub use request::Request;
use serde_json::Value;
pub use state::*;
use std::fmt::Display;

/// The JSON-RPC protocol version supported by this client.
///
/// Always set to `"2.0"` per the Electrum protocol specification.
pub const JSONRPC_VERSION_2_0: &str = "2.0";

/// An owned or borrowed static string.
pub type CowStr = std::borrow::Cow<'static, str>;

/// A double SHA256 hash (`sha256d`) used for Merkle branches and header proofs.
pub type DoubleSHA = bitcoin::hashes::sha256d::Hash;

/// A method name and its corresponding parameters, as used in a JSON-RPC request.
pub type MethodAndParams = (CowStr, Vec<Value>);

/// A server response that is either a success (`Ok`) or a JSON-RPC error (`Err`).
pub type ResponseResult<Resp> = Result<Resp, ResponseError>;

/// Internal type aliases for asynchronous client components.
mod async_aliases {
    use super::*;
    use futures::channel::{
        mpsc::{TrySendError, UnboundedReceiver, UnboundedSender},
        oneshot::{Receiver, Sender},
    };
    use pending_request::AsyncPendingRequest;

    /// Internal [`State`] instance specialized for tracking asynchronous requests.
    ///
    /// Used by the async client to associate incoming responses with pending requests.
    pub type AsyncState = State<AsyncPendingRequest>;

    /// The sending half of the channel used to enqueue one or more requests from [`AsyncClient`].
    ///
    /// These requests are processed and forwarded to [`State::track_request`] to be assigned an ID and serialized.
    pub type AsyncRequestSender = UnboundedSender<MaybeBatch<AsyncPendingRequest>>;

    /// The receiving half of the request channel used internally by the async client.
    ///
    /// Requests sent by [`AsyncClient`] are dequeued here and forwarded to [`State::track_request`].
    pub type AsyncRequestReceiver = UnboundedReceiver<MaybeBatch<AsyncPendingRequest>>;

    /// The error returned by [`AsyncClient::send_request`] when a request fails.
    ///
    /// This may occur if the server responds with an error, the request is canceled, or the client is shut down.
    pub type AsyncRequestError = request::Error<AsyncRequestSendError>;

    /// The error that occurs when a request cannot be sent into the async request channel.
    ///
    /// This typically means the client’s background task has shut down or the queue is disconnected.
    pub type AsyncRequestSendError = TrySendError<MaybeBatch<AsyncPendingRequest>>;

    /// A oneshot sender used to deliver the result of a tracked async request.
    ///
    /// Used internally by the client to fulfill the future returned by [`AsyncBatchRequest::request`].
    pub type AsyncResponseSender<Resp> = Sender<ResponseResult<Resp>>;

    /// A oneshot receiver used to await the result of a tracked async request.
    ///
    /// Awaiting this will yield the final response or error once the server replies.
    pub type AsyncResponseReceiver<Resp> = Receiver<ResponseResult<Resp>>;

    /// The sending half of the internal event stream, used to emit [`Event`]s from the client worker loop.
    pub type AsyncEventSender = UnboundedSender<Event>;

    /// The receiving half of the internal event stream, returned to users of [`AsyncClient`].
    ///
    /// This yields all incoming [`Event`]s from the Electrum server, including notifications and responses.
    pub type AsyncEventReceiver = UnboundedReceiver<Event>;
}
pub use async_aliases::*;

/// Internal type aliases for blocking client components.
mod blocking_aliases {
    use super::*;
    use pending_request::BlockingPendingRequest;
    use std::sync::mpsc::{Receiver, SendError, Sender, SyncSender};

    /// Internal [`State`] specialized for tracking blocking requests.
    pub type BlockingState = State<BlockingPendingRequest>;

    /// Channel sender for sending blocking requests from [`BlockingClient`] to the write thread.
    pub type BlockingRequestSender = Sender<MaybeBatch<BlockingPendingRequest>>;

    /// Channel receiver used by the write thread to dequeue pending requests.
    pub type BlockingRequestReceiver = Receiver<MaybeBatch<BlockingPendingRequest>>;

    /// Error returned by [`BlockingClient::send_request`] if the request fails or is canceled.
    pub type BlockingRequestError = request::Error<BlockingRequestSendError>;

    /// Error that occurs when a blocking request cannot be sent to the internal request channel.
    ///
    /// Typically indicates that the client has been shut down.
    pub type BlockingRequestSendError = SendError<MaybeBatch<BlockingPendingRequest>>;

    /// One-shot sender used to deliver the result of a tracked blocking request.
    pub type BlockingResponseSender<Resp> = SyncSender<ResponseResult<Resp>>;

    /// One-shot receiver used to block and wait for a response to a tracked request.
    pub type BlockingResponseReceiver<Resp> = Receiver<ResponseResult<Resp>>;

    /// Channel sender used by the read thread to emit [`Event`]s.
    pub type BlockingEventSender = Sender<Event>;

    /// Channel receiver used to receive [`Event`]s from the Electrum server.
    pub type BlockingEventReceiver = Receiver<Event>;
}
pub use blocking_aliases::*;

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
/// This is the server’s response to a client-issued request. It may contain either a `result`
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
    pub id: usize,

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
pub enum RawNotificationOrResponse {
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
    pub id: usize,

    /// The method to be invoked (e.g., `"blockchain.headers.subscribe"`).
    pub method: CowStr,

    /// The parameters passed to the method.
    pub params: Vec<Value>,
}

impl RawRequest {
    /// Constructs a new JSON-RPC request with the given ID, method, and parameters.
    ///
    /// This sets the JSON-RPC version to `"2.0"`.
    pub fn new(id: usize, method: CowStr, params: Vec<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION_2_0.into(),
            id,
            method,
            params,
        }
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
pub enum MaybeBatch<T> {
    Single(T),
    Batch(Vec<T>),
}

impl<T> MaybeBatch<T> {
    /// Converts this `MaybeBatch` into a `Vec<T>`.
    ///
    /// If it is a `Single`, returns a one-element vector. If it is a `Batch`, returns the inner vector.
    pub fn into_vec(self) -> Vec<T> {
        match self {
            MaybeBatch::Single(item) => vec![item],
            MaybeBatch::Batch(batch) => batch,
        }
    }

    /// Pushes a new item into the given `Option<MaybeBatch<T>>`, creating or extending the batch.
    ///
    /// If the option is `None`, it becomes `Some(Single(item))`. If it already contains a value,
    /// it is converted into a `Batch` and the item is appended.
    pub fn push_opt(opt: &mut Option<Self>, item: T) {
        *opt = match opt.take() {
            None => Some(Self::Single(item)),
            Some(maybe_batch) => {
                let mut items = maybe_batch.into_vec();
                items.push(item);
                Some(MaybeBatch::Batch(items))
            }
        }
    }
}

impl<T> From<T> for MaybeBatch<T> {
    fn from(value: T) -> Self {
        Self::Single(value)
    }
}

impl<T> From<Vec<T>> for MaybeBatch<T> {
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
