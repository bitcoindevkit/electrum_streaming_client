use crate::{AsyncResponseSender, BlockingResponseSender, MethodAndParams, Request, ResponseError};

/// A tracked or untracked asynchronous request, paired with an optional response sender.
///
/// If `Some(sender)` is present, the response will be delivered through it.
/// If `None`, the response is expected to be emitted as an [`Event`] instead.
///
/// [`Event`]: crate::Event
pub type AsyncPendingRequestTuple<Req, Resp> = (Req, Option<AsyncResponseSender<Resp>>);

/// A tracked or untracked blocking request, paired with an optional response sender.
///
/// If `Some(sender)` is present, the response will be sent through it.
/// If `None`, the response is expected to be emitted as an [`Event`] instead.
///
/// [`Event`]: crate::Event
pub type BlockingPendingRequestTuple<Req, Resp> = (Req, Option<BlockingResponseSender<Resp>>);

macro_rules! gen_pending_request_types {
    ($($name:ident),*) => {
        /// A successfully handled request and its decoded server response.
        ///
        /// This enum is returned when a request has been fully processed and the server replied
        /// with a valid `result`. It contains both the original request and the corresponding
        /// response.
        ///
        /// `SatisfiedRequest` is used by the [`Event::Response`] variant to expose typed
        /// request-response pairs to the caller.
        ///
        /// You typically don’t construct this manually — it is created internally by the client
        /// after decoding JSON-RPC responses.
        ///
        /// [`Event::Response`]: crate::Event::Response
        #[derive(Debug, Clone)]
        pub enum SatisfiedRequest {
            $($name {
                req: crate::request::$name,
                resp: <crate::request::$name as Request>::Response,
            }),*,
        }

        /// A request that received an error response from the Electrum server.
        ///
        /// This enum represents a completed request where the server returned a JSON-RPC error
        /// instead of a `result`. It contains both the original request and the associated error.
        ///
        /// This is used by the [`Event::ResponseError`] variant to expose server-side failures
        /// in a typed manner.
        ///
        /// Like [`SatisfiedRequest`], this is created internally by the client during response
        /// processing.
        ///
        /// [`Event::ResponseError`]: crate::Event::ResponseError
        #[derive(Debug, Clone)]
        pub enum ErroredRequest {
            $($name {
                req: crate::request::$name,
                error: ResponseError,
            }),*,
        }

        impl core::fmt::Display for ErroredRequest {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(Self::$name { req, error } => write!(f, "Server responsed to {:?} with error: {}", req, error)),*,
                }
            }
        }

        impl std::error::Error for ErroredRequest {}

        /// A trait representing a request that has been sent to the Electrum server and is awaiting
        /// a response.
        ///
        /// This trait is used internally to track the lifecycle of a request, including:
        /// - extracting its method and parameters before sending,
        /// - handling a successful server response,
        /// - handling an error response.
        ///
        /// Both [`AsyncPendingRequest`] and [`BlockingPendingRequest`] implement this trait.
        /// These are generated enums that hold the original request and, optionally, a response
        /// channel.
        ///
        /// You should not implement this trait manually — it is only used inside the client engine
        /// for matching raw Electrum responses to typed results.
        ///
        /// [`AsyncPendingRequest`]: crate::pending_request::AsyncPendingRequest
        /// [`BlockingPendingRequest`]: crate::pending_request::BlockingPendingRequest
        pub trait PendingRequest {
            /// Returns the Electrum method name and parameters for this request.
            ///
            /// This is used to serialize the request into a JSON-RPC message before sending it to
            /// the server.
            ///
            /// The method and parameters must match the format expected by the Electrum protocol.
            fn to_method_and_params(&self) -> MethodAndParams;

            /// Attempts to decode a successful server response (`result`) into a typed value.
            ///
            /// This is called when a matching response arrives from the server. If the request was
            /// tracked, this method deserializes the response and either:
            /// - completes the associated response channel (if present), or
            /// - returns a [`SatisfiedRequest`] directly, if untracked.
            ///
            /// Returns an error if deserialization fails.
            ///
            /// [`SatisfiedRequest`]: crate::SatisfiedRequest
            fn satisfy(self, raw_resp: serde_json::Value) -> Result<Option<SatisfiedRequest>, serde_json::Error>;

            /// Handles a server-side error response (`error`) for this request.
            ///
            /// If the request was tracked, this sends the error through the associated response
            /// channel. Otherwise, it returns a [`ErroredRequest`] containing the original request
            /// and the error.
            ///
            /// [`ErroredRequest`]: crate::ErroredRequest
            fn satisfy_error(self, raw_error: serde_json::Value) -> Option<ErroredRequest>;
        }

        /// An internal representation of a pending asynchronous Electrum request.
        ///
        /// Each variant corresponds to a specific request type. The enum holds:
        /// - the original request (`req`), and
        /// - an optional response channel (`resp_tx`) that will be completed once a server response
        ///   is received.
        ///
        /// This type is created when calling [`AsyncBatchRequest::request`] or
        /// [`AsyncBatchRequest::event_request`], and is consumed by the client when processing
        /// responses.
        ///
        /// If `resp_tx` is present, the request is tracked and its response will complete the
        /// associated future. If `resp_tx` is `None`, the response will be delivered as an
        /// [`Event`] instead.
        ///
        /// You typically don’t construct this type directly — it is produced by the batch builder
        /// or macros.
        ///
        /// [`AsyncBatchRequest::request`]: crate::AsyncBatchRequest::request
        /// [`AsyncBatchRequest::event_request`]: crate::AsyncBatchRequest::event_request
        /// [`Event`]: crate::Event
        #[derive(Debug)]
        pub enum AsyncPendingRequest {
            $($name {
                req: crate::request::$name,
                resp_tx: Option<AsyncResponseSender<<crate::request::$name as Request>::Response>>,
            }),*,
        }

        $(
            impl From<AsyncPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>> for AsyncPendingRequest {
                fn from((req, resp_tx): AsyncPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>) -> Self {
                    Self::$name{ req, resp_tx }
                }
            }
        )*

        impl PendingRequest for AsyncPendingRequest {
            fn to_method_and_params(&self) -> MethodAndParams {
                match self {
                    $(AsyncPendingRequest::$name{ req, .. } => req.to_method_and_params()),*
                }
            }

            fn satisfy(self, raw_resp: serde_json::Value) -> Result<Option<SatisfiedRequest>, serde_json::Error> {
                use crate::request;
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        let resp = serde_json::from_value::<<request::$name as Request>::Response>(raw_resp)?;
                        Ok(match resp_tx {
                            Some(tx) => {
                                let _ = tx.send(Ok(resp));
                                None
                            }
                            None => Some(SatisfiedRequest::$name { req, resp }),
                        })
                    }),*
                }
            }

            fn satisfy_error(self, raw_error: serde_json::Value) -> Option<ErroredRequest> {
                let error = ResponseError(raw_error);
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        match resp_tx {
                            Some(tx) => { let _ = tx.send(Err(error)); None }
                            None => Some(ErroredRequest::$name{ req, error }),
                        }
                    }),*
                }
            }
        }

        /// An internal representation of a pending blocking Electrum request.
        ///
        /// Each variant corresponds to a specific request type. The enum holds:
        /// - the original request (`req`), and
        /// - an optional response channel (`resp_tx`) that will be fulfilled once a server response
        ///   is received.
        ///
        /// This type is created when calling [`BlockingBatchRequest::request`] or
        /// [`BlockingBatchRequest::event_request`], and is consumed by the client when processing
        /// server responses.
        ///
        /// If `resp_tx` is present, the request is tracked and the response will be sent through
        /// the associated receiver. If `resp_tx` is `None`, the response will be delivered as an
        /// [`Event`] instead.
        ///
        /// This type is used internally by the blocking client and is typically not constructed
        /// directly.
        ///
        /// [`BlockingBatchRequest::request`]: crate::BlockingBatchRequest::request
        /// [`BlockingBatchRequest::event_request`]: crate::BlockingBatchRequest::event_request
        /// [`Event`]: crate::Event
        #[derive(Debug)]
        pub enum BlockingPendingRequest {
            $($name {
                req: crate::request::$name,
                resp_tx: Option<BlockingResponseSender<<crate::request::$name as Request>::Response>>,
            }),*,
        }

        $(
            impl From<BlockingPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>> for BlockingPendingRequest {
                fn from((req, resp_tx): BlockingPendingRequestTuple<crate::request::$name, <crate::request::$name as Request>::Response>) -> Self {
                    Self::$name{ req, resp_tx }
                }
            }
        )*

        impl PendingRequest for BlockingPendingRequest {
            fn to_method_and_params(&self) -> MethodAndParams {
                match self {
                    $(BlockingPendingRequest::$name{ req, .. } => req.to_method_and_params()),*
                }
            }

            fn satisfy(self, raw_resp: serde_json::Value) -> Result<Option<SatisfiedRequest>, serde_json::Error> {
                use crate::request;
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        let resp = serde_json::from_value::<<request::$name as Request>::Response>(raw_resp)?;
                        Ok(match resp_tx {
                            Some(tx) => {
                                let _ = tx.send(Ok(resp));
                                None
                            }
                            None => Some(SatisfiedRequest::$name { req, resp }),
                        })
                    }),*
                }
            }

            fn satisfy_error(self, raw_error: serde_json::Value) -> Option<ErroredRequest> {
                let error = ResponseError(raw_error);
                match self {
                    $(Self::$name{ req, resp_tx } => {
                        match resp_tx {
                            Some(tx) => { let _ = tx.send(Err(error)); None }
                            None => Some(ErroredRequest::$name{ req, error }),
                        }
                    }),*
                }
            }
        }
    };
}

gen_pending_request_types! {
    Header,
    HeaderWithProof,
    Headers,
    HeadersWithCheckpoint,
    EstimateFee,
    HeadersSubscribe,
    RelayFee,
    GetBalance,
    GetHistory,
    GetMempool,
    ListUnspent,
    ScriptHashSubscribe,
    ScriptHashUnsubscribe,
    BroadcastTx,
    GetTx,
    GetTxMerkle,
    GetTxidFromPos,
    GetFeeHistogram,
    Banner,
    Ping,
    Custom
}

impl<A: PendingRequest> PendingRequest for Box<A> {
    fn to_method_and_params(&self) -> MethodAndParams {
        self.as_ref().to_method_and_params()
    }

    fn satisfy(
        self,
        raw_resp: serde_json::Value,
    ) -> Result<Option<SatisfiedRequest>, serde_json::Error> {
        (*self).satisfy(raw_resp)
    }

    fn satisfy_error(self, raw_error: serde_json::Value) -> Option<ErroredRequest> {
        (*self).satisfy_error(raw_error)
    }
}
