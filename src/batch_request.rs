use crate::*;

#[must_use]
#[derive(Debug, Default)]
pub struct AsyncBatchRequest {
    inner: Option<MaybeBatch<AsyncPendingRequest>>,
}

impl AsyncBatchRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_inner(self) -> Option<MaybeBatch<AsyncPendingRequest>> {
        self.inner
    }

    /// Add a `request` to the batch.
    ///
    /// Returns a future which becomes ready once this single request is satisfied.
    ///
    /// The returned future will block forever until the [`AsyncBatchRequest`] is sent via
    /// [`AsyncClient::send_batch`].
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

    /// Add a `request` to the batch where we want to receive the response as an event.
    pub fn event_request<Req>(&mut self, request: Req)
    where
        Req: Request,
        AsyncPendingRequestTuple<Req, Req::Response>: Into<AsyncPendingRequest>,
    {
        MaybeBatch::push_opt(&mut self.inner, (request, None).into());
    }
}

#[must_use]
#[derive(Debug, Default)]
pub struct BlockingBatchRequest {
    inner: Option<MaybeBatch<BlockingPendingRequest>>,
}

impl BlockingBatchRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_inner(self) -> Option<MaybeBatch<BlockingPendingRequest>> {
        self.inner
    }

    /// Add a `request` to the batch.
    pub fn request<Req>(&mut self, req: Req) -> BlockingResponseReceiver<Req::Response>
    where
        Req: Request,
        BlockingPendingRequestTuple<Req, Req::Response>: Into<BlockingPendingRequest>,
    {
        let (resp_tx, resp_rx) = std::sync::mpsc::sync_channel(1);
        MaybeBatch::push_opt(&mut self.inner, (req, Some(resp_tx)).into());
        resp_rx
    }

    /// Add a `request` to the batch where we want to receive the response as an event.
    pub fn event_request<Req>(&mut self, request: Req)
    where
        Req: Request,
        BlockingPendingRequestTuple<Req, Req::Response>: Into<BlockingPendingRequest>,
    {
        MaybeBatch::push_opt(&mut self.inner, (request, None).into());
    }
}

#[derive(Debug)]
pub enum BatchRequestError {
    Canceled,
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
