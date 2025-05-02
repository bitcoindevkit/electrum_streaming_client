use crate::*;

/// A builder for batching multiple asynchronous requests to the Electrum server.
///
/// This type allows queuing both:
/// - tracked requests via [`request`] (which return a [`Future`] that resolves to a response), and
/// - event-style requests via [`event_request`] (which emit [`Event`]s through the
///   [`AsyncEventReceiver`] instead of a future).
///
/// After building the batch, submit it using [`AsyncClient::send_batch`]. The batch will be
/// converted into a raw JSON-RPC message and sent to the server.
///
/// **Important:** Do not `.await` any futures returned by [`request`] until *after* the batch has
/// been sent. Doing so will cause the future to block indefinitely, as the request ID is not yet
/// assigned and the response cannot be matched.
///
/// This type is useful for reducing round-trips and issuing dependent or related requests together.
///
/// [`request`]: Self::request
/// [`event_request`]: Self::event_request
/// [`Future`]: core::future::Future
/// [`AsyncClient::send_batch`]: crate::AsyncClient::send_batch
/// [`AsyncEventReceiver`]: crate::AsyncEventReceiver
/// [`Event`]: crate::Event
#[must_use]
#[derive(Debug, Default)]
pub struct AsyncBatchRequest {
    inner: Option<MaybeBatch<AsyncPendingRequest>>,
}

impl AsyncBatchRequest {
    /// Creates a new empty async batch request builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes the batch and returns its raw contents, if any requests were added.
    ///
    /// Returns `Some` if the batch is non-empty, or `None` if it was empty.
    ///
    /// This is used internally by [`AsyncClient::send_batch`] to extract the batched request set.
    ///
    /// [`AsyncClient::send_batch`]: crate::AsyncClient::send_batch
    pub fn into_inner(self) -> Option<MaybeBatch<AsyncPendingRequest>> {
        self.inner
    }

    /// Adds a tracked request to the batch and returns a [`Future`] that resolves to the response.
    ///
    /// This request will be tracked internally. The returned future must only be `.await`ed
    /// *after* the batch has been submitted with [`AsyncClient::send_batch`]. Awaiting too early
    /// will block forever.
    ///
    /// # Errors
    /// Returns an error if the request could not be added (e.g., duplicate or overflow).
    ///
    /// [`Future`]: futures::Future
    /// [`AsyncClient::send_batch`]: crate::AsyncClient::send_batch
    pub fn request<Req>(
        &mut self,
        req: Req,
    ) -> impl std::future::Future<Output = Result<Req::Response, BatchRequestError>>
           + Send
           + Sync
           + 'static
    where
        Req: Request,
        AsyncPendingRequestTuple<Req, Req::Response>: Into<AsyncPendingRequest>,
    {
        let (resp_tx, resp_rx) = futures::channel::oneshot::channel();
        MaybeBatch::push_opt(&mut self.inner, (req, Some(resp_tx)).into());
        async move {
            resp_rx
                .await
                .map_err(|_| BatchRequestError::Canceled)?
                .map_err(BatchRequestError::Response)
        }
    }

    /// Adds an event-style request to the batch.
    ///
    /// These requests do not return a future and will not be tracked internally. Any server
    /// response (including the initial result and any future notifications) will be delivered as
    /// [`Event`]s through the [`AsyncEventReceiver`] stream.
    ///
    /// Use this for subscription-style RPCs where responses should be handled uniformly as events.
    ///
    /// [`Event`]: crate::Event
    /// [`AsyncEventReceiver`]: crate::AsyncEventReceiver
    pub fn event_request<Req>(&mut self, request: Req)
    where
        Req: Request,
        AsyncPendingRequestTuple<Req, Req::Response>: Into<AsyncPendingRequest>,
    {
        MaybeBatch::push_opt(&mut self.inner, (request, None).into());
    }
}

/// A builder for batching multiple blocking requests to the Electrum server.
///
/// This type allows queuing both:
/// - tracked requests via [`request`] (which return blocking receivers for the responses), and
/// - event-style requests via [`event_request`] (which emit [`Event`]s through the
///   [`BlockingEventReceiver`] instead of a response handle).
///
/// After building the batch, submit it using [`BlockingClient::send_batch`]. The batch will be
/// serialized and sent to the server in a single write.
///
/// **Important:** Do not call `.recv()` on any response receivers returned by [`request`] until
/// *after* the batch has been sent. Receiving early will block forever, as the request has not yet
/// been transmitted and the ID not assigned.
///
/// [`request`]: Self::request
/// [`event_request`]: Self::event_request
/// [`BlockingClient::send_batch`]: crate::BlockingClient::send_batch
/// [`BlockingEventReceiver`]: crate::BlockingEventReceiver
/// [`Event`]: crate::Event
#[must_use]
#[derive(Debug, Default)]
pub struct BlockingBatchRequest {
    inner: Option<MaybeBatch<BlockingPendingRequest>>,
}

impl BlockingBatchRequest {
    /// Creates a new empty blocking batch request builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes the batch and returns its raw contents, if any requests were added.
    ///
    /// Returns `Some` if the batch is non-empty, or `None` if it was empty.
    ///
    /// This is used internally by [`BlockingClient::send_batch`] to extract the batched request set.
    ///
    /// [`BlockingClient::send_batch`]: crate::BlockingClient::send_batch
    pub fn into_inner(self) -> Option<MaybeBatch<BlockingPendingRequest>> {
        self.inner
    }

    /// Adds a tracked request to the batch and returns a receiver for the response.
    ///
    /// This request will be tracked internally. The returned receiver must only be used
    /// *after* the batch has been submitted with [`BlockingClient::send_batch`].
    /// Calling `.recv()` or `.wait()` too early will block indefinitely.
    ///
    /// # Errors
    /// Returns an error if the request could not be added (e.g., duplicate or overflow).
    ///
    /// [`BlockingClient::send_batch`]: crate::BlockingClient::send_batch
    pub fn request<Req>(&mut self, req: Req) -> BlockingResponseReceiver<Req::Response>
    where
        Req: Request,
        BlockingPendingRequestTuple<Req, Req::Response>: Into<BlockingPendingRequest>,
    {
        let (resp_tx, resp_rx) = std::sync::mpsc::sync_channel(1);
        MaybeBatch::push_opt(&mut self.inner, (req, Some(resp_tx)).into());
        resp_rx
    }

    /// Adds an event-style request to the batch.
    ///
    /// These requests do not return a receiver and will not be tracked internally. Any server
    /// response (including the initial result and any future notifications) will be delivered as
    /// [`Event`]s through the [`BlockingEventReceiver`] stream.
    ///
    /// Use this for subscription-style RPCs where responses should be handled uniformly as events.
    ///
    /// [`Event`]: crate::Event
    /// [`BlockingEventReceiver`]: crate::BlockingEventReceiver
    pub fn event_request<Req>(&mut self, request: Req)
    where
        Req: Request,
        BlockingPendingRequestTuple<Req, Req::Response>: Into<BlockingPendingRequest>,
    {
        MaybeBatch::push_opt(&mut self.inner, (request, None).into());
    }
}

/// An error that can occur when adding a request to a batch or polling its result.
///
/// This error is returned by [`AsyncBatchRequest::request`] or [`BlockingBatchRequest::request`]
/// when the future or receiver representing the response cannot complete.
///
/// It typically indicates that the batch was dropped, the client shut down, or the request
/// failed to be processed internally.
#[derive(Debug)]
pub enum BatchRequestError {
    /// The request was canceled before a response was received.
    ///
    /// This can occur if the client shuts down or if the request is dropped internally.
    Canceled,

    /// The server returned a response error.
    ///
    /// This indicates that the Electrum server replied with an error object, rather than a result.
    Response(ResponseError),
}

impl std::fmt::Display for BatchRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canceled => write!(f, "Request was canceled before being satisfied."),
            Self::Response(e) => write!(f, "Request satisfied with error: {}", e),
        }
    }
}

impl std::error::Error for BatchRequestError {}
