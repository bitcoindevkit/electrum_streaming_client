use crate::*;

#[derive(Debug, Clone)]
pub struct AsyncClient {
    tx: AsyncRequestSender,
}

impl From<AsyncRequestSender> for AsyncClient {
    fn from(tx: AsyncRequestSender) -> Self {
        Self { tx }
    }
}

impl AsyncClient {
    pub fn new<R, W>(
        reader: R,
        mut writer: W,
    ) -> (
        Self,
        AsyncEventReceiver,
        impl std::future::Future<Output = std::io::Result<()>> + Send,
    )
    where
        R: futures::AsyncRead + Send + Unpin,
        W: futures::AsyncWrite + Send + Unpin,
    {
        use futures::{channel::mpsc, StreamExt};
        let (event_tx, event_recv) = mpsc::unbounded::<Event>();
        let (req_tx, mut req_recv) = mpsc::unbounded::<MaybeBatch<AsyncPendingRequest>>();

        let mut incoming_stream =
            crate::io::ReadStreamer::new(futures::io::BufReader::new(reader)).fuse();
        let mut state = State::<AsyncPendingRequest>::new(0);

        let fut = async move {
            loop {
                futures::select! {
                    req_opt = req_recv.next() => match req_opt {
                        Some(req) => {
                            let raw_req = state.add_request(req);
                            crate::io::async_write(&mut writer, raw_req).await?;
                        },
                        None => break,
                    },
                    incoming_opt = incoming_stream.next() => match incoming_opt {
                        Some(incoming_res) => {
                            let event_opt = state
                                .consume(incoming_res?)
                                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
                            if let Some(event) = event_opt {
                                if let Err(_err) = event_tx.unbounded_send(event) {
                                    break;
                                }
                            }
                        },
                        None => break,
                    }
                }
            }
            std::io::Result::<()>::Ok(())
        };

        (Self { tx: req_tx }, event_recv, fut)
    }

    #[cfg(feature = "tokio")]
    pub fn new_tokio<R, W>(
        reader: R,
        writer: W,
    ) -> (
        Self,
        AsyncEventReceiver,
        impl std::future::Future<Output = std::io::Result<()>> + Send,
    )
    where
        R: tokio::io::AsyncRead + Send + Unpin,
        W: tokio::io::AsyncWrite + Send + Unpin,
    {
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
        Self::new(reader.compat(), writer.compat_write())
    }

    pub async fn send_request<Req>(&self, req: Req) -> Result<Req::Response, AsyncRequestError>
    where
        Req: Request,
        AsyncPendingRequestTuple<Req, Req::Response>: Into<AsyncPendingRequest>,
    {
        use futures::TryFutureExt;
        let mut batch = AsyncBatchRequest::new();
        let resp_fut = batch.request(req).map_err(|e| match e {
            BatchRequestError::Canceled => AsyncRequestError::Canceled,
            BatchRequestError::Response(e) => AsyncRequestError::Response(e),
        });
        self.send_batch(batch)
            .map_err(AsyncRequestError::Dispatch)?;
        resp_fut.await
    }

    pub fn send_event_request<Req>(&self, request: Req) -> Result<(), AsyncRequestSendError>
    where
        Req: Request,
        AsyncPendingRequestTuple<Req, Req::Response>: Into<AsyncPendingRequest>,
    {
        let mut batch = AsyncBatchRequest::new();
        batch.event_request(request);
        self.send_batch(batch)?;
        Ok(())
    }

    pub fn send_batch(&self, batch_req: AsyncBatchRequest) -> Result<bool, AsyncRequestSendError> {
        match batch_req.into_inner() {
            Some(batch) => self.tx.unbounded_send(batch).map(|_| true),
            None => Ok(false),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockingClient {
    tx: BlockingRequestSender,
}

impl From<BlockingRequestSender> for BlockingClient {
    fn from(tx: BlockingRequestSender) -> Self {
        Self { tx }
    }
}

impl BlockingClient {
    pub fn new<R, W>(
        reader: R,
        mut writer: W,
    ) -> (
        Self,
        BlockingEventReceiver,
        std::thread::JoinHandle<std::io::Result<()>>,
        std::thread::JoinHandle<std::io::Result<()>>,
    )
    where
        R: std::io::Read + Send + 'static,
        W: std::io::Write + Send + 'static,
    {
        use std::sync::mpsc::*;
        let (event_tx, event_recv) = channel::<Event>();
        let (req_tx, req_recv) = channel::<MaybeBatch<BlockingPendingRequest>>();
        let incoming_stream = crate::io::ReadStreamer::new(std::io::BufReader::new(reader));
        let read_state = std::sync::Arc::new(std::sync::Mutex::new(
            State::<BlockingPendingRequest>::new(0),
        ));
        let write_state = std::sync::Arc::clone(&read_state);

        let read_join = std::thread::spawn(move || -> std::io::Result<()> {
            for incoming_res in incoming_stream {
                let event_opt = read_state
                    .lock()
                    .unwrap()
                    .consume(incoming_res?)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
                if let Some(event) = event_opt {
                    if let Err(_err) = event_tx.send(event) {
                        break;
                    }
                }
            }
            Ok(())
        });
        let write_join = std::thread::spawn(move || -> std::io::Result<()> {
            for req in req_recv {
                let raw_req = write_state.lock().unwrap().add_request(req);
                crate::io::blocking_write(&mut writer, raw_req)?;
            }
            Ok(())
        });
        (Self { tx: req_tx }, event_recv, read_join, write_join)
    }

    pub fn send_request<Req>(&self, req: Req) -> Result<Req::Response, BlockingRequestError>
    where
        Req: Request,
        BlockingPendingRequestTuple<Req, Req::Response>: Into<BlockingPendingRequest>,
    {
        let mut batch = BlockingBatchRequest::new();
        let resp_rx = batch.request(req);
        self.send_batch(batch)
            .map_err(BlockingRequestError::Dispatch)?;
        resp_rx
            .recv()
            .map_err(|_| BlockingRequestError::Canceled)?
            .map_err(BlockingRequestError::Response)
    }

    pub fn send_event_request<Req>(&self, request: Req) -> Result<(), BlockingRequestSendError>
    where
        Req: Request,
        BlockingPendingRequestTuple<Req, Req::Response>: Into<BlockingPendingRequest>,
    {
        let mut batch = BlockingBatchRequest::new();
        batch.event_request(request);
        self.send_batch(batch)?;
        Ok(())
    }

    pub fn send_batch(
        &self,
        batch_req: BlockingBatchRequest,
    ) -> Result<bool, BlockingRequestSendError> {
        match batch_req.into_inner() {
            Some(batch) => self.tx.send(batch).map(|_| true),
            None => Ok(false),
        }
    }
}
