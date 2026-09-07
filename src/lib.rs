#![doc = include_str!("../README.md")]
pub mod client;
pub use client::{
    AsyncClient, AsyncRequestError, AsyncRequestSendError, BlockingClient, BlockingRequestError,
    BlockingRequestSendError,
};
mod custom_serde;
mod hash_types;
pub mod io;
pub mod notification;
mod pending_request;
pub mod protocol;
pub mod request;
mod request_tracker;
pub mod response;
pub mod transport;
pub use hash_types::*;
pub use pending_request::*;
pub use protocol::*;
pub use request::Request;
pub use request_tracker::*;
pub use serde_json;
pub use transport::{
    ConnectConfig, ConnectConfigBuilder, ConnectError, Host, ParseServerAddrError, ServerAddr,
};

#[cfg(feature = "ssl")]
pub use transport::TlsError;

/// An owned or borrowed static string.
pub type CowStr = std::borrow::Cow<'static, str>;

/// A double SHA256 hash (`sha256d`) used for Merkle branches and header proofs.
pub type DoubleSHA = bitcoin::hashes::sha256d::Hash;

/// A method name and its corresponding parameters, as used in a JSON-RPC request.
pub type MethodAndParams = (CowStr, Vec<serde_json::Value>);

/// A server response that is either a success (`Ok`) or a JSON-RPC error (`Err`).
pub type ResponseResult<Resp> = Result<Resp, ResponseError>;
