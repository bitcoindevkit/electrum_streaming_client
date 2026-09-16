use crate::{Event, MethodAndParams, Request, ResponseError, ResponseResult};
use serde_json::Value;

/// Extension trait for request types that can construct [`CompletedRequest`] and [`FailedRequest`].
///
/// This trait is automatically implemented for all built-in request types via the
/// `gen_pending_request_types!` macro. It bridges a typed request to the enum variants used in
/// [`Event`].
///
/// [`Event`]: crate::Event
pub trait RequestExt: Request + Sized {
    /// Wraps this request and its decoded response into a [`CompletedRequest`].
    fn into_completed(self, resp: Self::Response) -> CompletedRequest;

    /// Wraps this request and an error into an [`FailedRequest`].
    fn into_failed(self, error: ResponseError) -> FailedRequest;
}

macro_rules! gen_pending_request_types {
    ($($name:ident),*) => {
        /// A successfully handled request and its decoded server response.
        ///
        /// This enum is returned when a request has been fully processed and the server replied
        /// with a valid `result`. It contains both the original request and the corresponding
        /// response.
        ///
        /// `CompletedRequest` is used by the [`Event::Response`] variant to expose typed
        /// request-response pairs to the caller.
        ///
        /// You typically don't construct this manually — it is created internally by the client
        /// after decoding JSON-RPC responses.
        ///
        /// [`Event::Response`]: crate::Event::Response
        #[derive(Debug, Clone)]
        pub enum CompletedRequest {
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
        /// Like [`CompletedRequest`], this is created internally by the client during response
        /// processing.
        ///
        /// [`Event::ResponseError`]: crate::Event::ResponseError
        #[derive(Debug, Clone)]
        pub enum FailedRequest {
            $($name {
                req: crate::request::$name,
                error: ResponseError,
            }),*,
        }

        impl core::fmt::Display for FailedRequest {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(Self::$name { req, error } => write!(f, "Server responsed to {:?} with error: {}", req, error)),*,
                }
            }
        }

        impl std::error::Error for FailedRequest {}

        $(
            impl RequestExt for crate::request::$name {
                fn into_completed(self, resp: <Self as Request>::Response) -> CompletedRequest {
                    CompletedRequest::$name { req: self, resp }
                }
                fn into_failed(self, error: ResponseError) -> FailedRequest {
                    FailedRequest::$name { req: self, error }
                }
            }
        )*
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
    BroadcastPackage,
    GetTx,
    GetTxMerkle,
    GetTxidFromPos,
    GetFeeHistogram,
    GetMempoolInfo,
    ServerVersion,
    Banner,
    Features,
    Ping,
    Custom
}

type Handler =
    Box<dyn FnOnce(Result<Value, Value>) -> Result<Option<Event>, serde_json::Error> + Send + Sync>;

/// A pending request that has been sent to the Electrum server and is awaiting a response.
///
/// This struct holds a type-erased handler closure that knows how to deserialize the server's
/// raw JSON response and either:
/// - dispatch it through a callback (for tracked requests), or
/// - construct an [`Event`] (for event-style requests).
///
/// Construct via [`PendingRequest::new`] (with a callback) or [`PendingRequest::event`] (without).
///
/// [`Event`]: crate::Event
pub struct PendingRequest {
    method_and_params: MethodAndParams,
    handler: Handler,
}

impl PendingRequest {
    /// Creates a new pending request with an optional typed callback.
    ///
    /// If `callback` is `Some`, the response will be deserialized and dispatched through it,
    /// and [`RequestTracker::handle_incoming`] will return `Ok(None)` for this request.
    ///
    /// If `callback` is `None`, the response will be wrapped in an [`Event`] and returned from
    /// [`RequestTracker::handle_incoming`].
    ///
    /// [`RequestTracker::handle_incoming`]: crate::RequestTracker::handle_incoming
    /// [`Event`]: crate::Event
    pub fn new<Req: RequestExt + Send + Sync + 'static>(
        req: Req,
        callback: Option<impl FnOnce(ResponseResult<Req::Response>) + Send + Sync + 'static>,
    ) -> Self {
        let method_and_params = req.to_method_and_params();
        Self {
            method_and_params,
            handler: Box::new(move |raw_result| match (raw_result, callback) {
                (Ok(raw_resp), Some(cb)) => {
                    let resp = serde_json::from_value(raw_resp)?;
                    cb(Ok(resp));
                    Ok(None)
                }
                (Ok(raw_resp), None) => {
                    let resp = serde_json::from_value(raw_resp)?;
                    Ok(Some(Event::Response(req.into_completed(resp))))
                }
                (Err(raw_err), Some(cb)) => {
                    cb(Err(ResponseError(raw_err)));
                    Ok(None)
                }
                (Err(raw_err), None) => Ok(Some(Event::ResponseError(
                    req.into_failed(ResponseError(raw_err)),
                ))),
            }),
        }
    }

    /// Creates a new pending request without a callback (event-style).
    ///
    /// The server's response will be returned as an [`Event`] from [`RequestTracker::handle_incoming`].
    ///
    /// [`RequestTracker::handle_incoming`]: crate::RequestTracker::handle_incoming
    /// [`Event`]: crate::Event
    pub fn event<Req: RequestExt + Send + Sync + 'static>(req: Req) -> Self {
        Self::new(req, None::<fn(ResponseResult<Req::Response>)>)
    }

    /// Returns the method name and parameters for this request.
    ///
    /// Used to serialize the request into a JSON-RPC message and to reconstruct
    /// [`RawRequest`]s for pending requests.
    ///
    /// [`RawRequest`]: crate::RawRequest
    pub fn to_method_and_params(&self) -> MethodAndParams {
        self.method_and_params.clone()
    }

    /// Handles the raw server result by invoking the type-erased handler closure.
    ///
    /// Returns `Ok(Some(Event))` for event-style requests, `Ok(None)` for callback-dispatched
    /// requests, or `Err` if deserialization failed.
    pub fn handle(self, result: Result<Value, Value>) -> Result<Option<Event>, serde_json::Error> {
        (self.handler)(result)
    }
}

impl std::fmt::Debug for PendingRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingRequest")
            .field("method_and_params", &self.method_and_params)
            .finish_non_exhaustive()
    }
}

/// A builder for batching multiple requests to the Electrum server.
///
/// This type allows queuing both:
/// - tracked requests via [`request`] (which take a callback to receive the typed response), and
/// - event-style requests via [`event_request`] (which emit [`Event`]s through the event receiver
///   instead of a callback).
///
/// After building the batch, submit it using [`AsyncClient::send_batch`] or
/// [`BlockingClient::send_batch`]. The batch will be converted into a raw JSON-RPC message and
/// sent to the server.
///
/// [`request`]: Self::request
/// [`event_request`]: Self::event_request
/// [`AsyncClient::send_batch`]: crate::AsyncClient::send_batch
/// [`BlockingClient::send_batch`]: crate::BlockingClient::send_batch
/// [`Event`]: crate::Event
#[must_use]
#[derive(Debug, Default)]
pub struct BatchRequest {
    inner: Option<crate::RawOneOrMany<PendingRequest>>,
}

impl BatchRequest {
    /// Creates a new empty batch request builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes the batch and returns its raw contents, if any requests were added.
    ///
    /// Returns `Some` if the batch is non-empty, or `None` if it was empty.
    pub fn into_inner(self) -> Option<crate::RawOneOrMany<PendingRequest>> {
        self.inner
    }

    /// Adds a tracked request to the batch with a typed callback.
    ///
    /// The callback will be invoked with the deserialized response (or error) once the server
    /// replies. The callback is type-erased internally, so it works for both async and blocking
    /// clients.
    pub fn request<Req, F>(&mut self, req: Req, callback: F)
    where
        Req: RequestExt + Send + Sync + 'static,
        F: FnOnce(ResponseResult<Req::Response>) + Send + Sync + 'static,
    {
        crate::RawOneOrMany::push_opt(&mut self.inner, PendingRequest::new(req, Some(callback)));
    }

    /// Adds a tracked request and returns an async receiver for the response.
    ///
    /// This is a convenience wrapper around [`request`](Self::request) that creates a
    /// [`futures::channel::oneshot`] channel internally. The returned receiver can be
    /// `.await`ed after the batch is sent.
    pub fn request_async<Req>(
        &mut self,
        req: Req,
    ) -> futures::channel::oneshot::Receiver<ResponseResult<Req::Response>>
    where
        Req: RequestExt + Send + Sync + 'static,
        Req::Response: Send,
    {
        let (tx, rx) = futures::channel::oneshot::channel();
        self.request(req, move |result| {
            let _ = tx.send(result);
        });
        rx
    }

    /// Adds a tracked request and returns a blocking receiver for the response.
    ///
    /// This is a convenience wrapper around [`request`](Self::request) that creates a
    /// [`std::sync::mpsc::sync_channel`] internally. The returned receiver can be used
    /// with [`recv`](std::sync::mpsc::Receiver::recv) after the batch is sent.
    pub fn request_blocking<Req>(
        &mut self,
        req: Req,
    ) -> std::sync::mpsc::Receiver<ResponseResult<Req::Response>>
    where
        Req: RequestExt + Send + Sync + 'static,
        Req::Response: Send,
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.request(req, move |result| {
            let _ = tx.send(result);
        });
        rx
    }

    /// Adds an event-style request to the batch.
    ///
    /// These requests do not take a callback. Any server response (including the initial result
    /// and any future notifications) will be delivered as [`Event`]s through the event receiver.
    ///
    /// Use this for subscription-style RPCs where responses should be handled uniformly as events.
    ///
    /// [`Event`]: crate::Event
    pub fn event_request<Req: RequestExt + Send + Sync + 'static>(&mut self, req: Req) {
        crate::RawOneOrMany::push_opt(&mut self.inner, PendingRequest::event(req));
    }
}
