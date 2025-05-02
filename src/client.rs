use crate::*;

/// An asynchronous Electrum client built on the [`futures`] I/O ecosystem.
///
/// This client allows sending JSON-RPC requests and receiving [`Event`]s from an Electrum server
/// over any transport that implements [`AsyncBufRead`] and [`AsyncWrite`].
///
/// To drive the client, you must poll the [`Future`] returned by [`AsyncClient::new`] or
/// [`AsyncClient::new_tokio`]. This worker future handles reading and writing to the transport,
/// parsing server responses, and routing them to the internal state and event stream.
///
/// Use the associated [`AsyncEventReceiver`] to receive [`Event`]s pushed by the server.
/// These may include responses to previous requests, or server-initiated notifications.
///
/// ### Constructors
/// - [`AsyncClient::new`] is runtime-agnostic and works with any `futures`-based transport.
/// - [`AsyncClient::new_tokio`] enables integration with `tokio`-based I/O types.
///
/// [`Future`]: futures::Future
/// [`Event`]: crate::Event
/// [`AsyncBufRead`]: futures::io::AsyncBufRead
/// [`AsyncWrite`]: futures::io::AsyncWrite
/// [`AsyncEventReceiver`]: crate::AsyncEventReceiver
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
    /// Creates a new [`AsyncClient`] using the given async reader and writer.
    ///
    /// This constructor supports any transport implementing [`futures::AsyncRead`] and
    /// [`futures::AsyncWrite`]. The client will handle request tracking, response matching, and
    /// notification delivery.
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - `AsyncClient`: the handle for sending requests.
    /// - [`AsyncEventReceiver`]: a stream of [`Event`]s emitted by the Electrum server.
    /// - A `Future`: the client worker loop. This must be polled (e.g., via `tokio::spawn`)
    ///   to drive the connection.
    ///
    /// [`AsyncEventReceiver`]: crate::AsyncEventReceiver
    /// [`Event`]: crate::Event
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
                            let raw_req = state.track_request(req);
                            crate::io::async_write(&mut writer, raw_req).await?;
                        },
                        None => break,
                    },
                    incoming_opt = incoming_stream.next() => match incoming_opt {
                        Some(incoming_res) => {
                            let event_opt = state
                                .process_incoming(incoming_res?)
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

    /// Creates a new [`AsyncClient`] using Tokio-based I/O types.
    ///
    /// This is a convenience constructor for users of the Tokio runtime. It accepts types
    /// implementing [`tokio::io::AsyncRead`] and [`tokio::io::AsyncWrite`], wraps them in
    /// compatibility adapters, and forwards them to [`AsyncClient::new`].
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - `AsyncClient`: the handle for sending requests.
    /// - [`AsyncEventReceiver`]: a stream of [`Event`]s emitted by the Electrum server.
    /// - A `Future`: the client worker loop. This must be spawned or polled to keep the client
    ///   alive.
    ///
    /// [`AsyncEventReceiver`]: crate::AsyncEventReceiver
    /// [`Event`]: crate::Event
    /// [`AsyncClient::new`]: crate::AsyncClient::new
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

    /// Sends a single tracked request to the Electrum server and awaits the response.
    ///
    /// This method is for request–response style interactions where only a single result is
    /// expected.
    ///
    /// # Errors
    /// Returns [`AsyncRequestError::Dispatch`] if sending fails, or [`AsyncRequestError::Response`]
    /// if the server replies with an error. If the request is canceled before completion, returns
    /// [`AsyncRequestError::Canceled`].
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

    /// Sends a request that is expected to result in an event-based response (e.g., a
    /// notification).
    ///
    /// Unlike [`send_request`], this method does not track or await a direct response. Instead, any
    /// resulting data will be emitted as an [`Event`] through the [`AsyncEventReceiver`] stream.
    ///
    /// This is useful for requests like `blockchain.headers.subscribe`, where the initial response
    /// and later notifications share the same structure and can be handled uniformly as events.
    ///
    /// # Errors
    ///
    /// Returns [`AsyncRequestSendError`] if the request could not be queued for sending.
    ///
    /// [`send_request`]: Self::send_request
    /// [`Event`]: crate::Event
    /// [`AsyncEventReceiver`]: crate::AsyncEventReceiver
    /// [`AsyncRequestSendError`]: crate::AsyncRequestSendError
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

    /// Sends a batch of requests to the Electrum server.
    ///
    /// The batch is constructed using [`AsyncBatchRequest`], which allows queuing both tracked
    /// requests (via [`AsyncBatchRequest::request`]) and event-style requests (via
    /// [`AsyncBatchRequest::event_request`]).
    ///
    /// Tracked requests return futures that resolve to the server’s response. Event-style requests
    /// (e.g., subscriptions) do produce an initial server response, but it is delivered through the
    /// [`AsyncEventReceiver`] and not through a dedicated future.
    ///
    /// **Important:** Do not `.await` any futures returned by [`AsyncBatchRequest::request`] until
    /// *after* the batch has been submitted via `send_batch`. Awaiting too early will block
    /// forever, as the requests haven’t been assigned IDs or sent yet.
    ///
    /// This method does not await any responses itself. Responses and notifications will be
    /// delivered asynchronously via the [`AsyncEventReceiver`] or via the [`Future`]s returned by
    /// [`AsyncBatchRequest::request`] — assuming they are awaited at the correct time.
    ///
    /// # Returns
    /// - `Ok(true)` if the batch was non-empty and sent successfully.
    /// - `Ok(false)` if the batch was empty and nothing was sent.
    /// - `Err` if the batch could not be sent (e.g., if the client was shut down).
    ///
    /// [`Future`]: futures::Future
    /// [`AsyncBatchRequest`]: crate::AsyncBatchRequest
    /// [`AsyncBatchRequest::request`]: crate::AsyncBatchRequest::request
    /// [`AsyncBatchRequest::event_request`]: crate::AsyncBatchRequest::event_request
    /// [`AsyncEventReceiver`]: crate::AsyncEventReceiver
    pub fn send_batch(&self, batch_req: AsyncBatchRequest) -> Result<bool, AsyncRequestSendError> {
        match batch_req.into_inner() {
            Some(batch) => self.tx.unbounded_send(batch).map(|_| true),
            None => Ok(false),
        }
    }
}

/// A blocking Electrum client built on standard I/O.
///
/// This client wraps a blocking transport implementing [`std::io::Read`] and [`std::io::Write`] and
/// provides an interface for sending requests and receiving [`Event`]s synchronously.
///
/// Internally, the client spawns two threads: one for reading from the server and one for writing.
/// These threads are started via [`BlockingClient::new`] and returned as `JoinHandle`s.
///
/// Use the associated [`BlockingEventReceiver`] to receive [`Event`]s emitted by the server.
///
/// [`Event`]: crate::Event
/// [`BlockingEventReceiver`]: crate::BlockingEventReceiver
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
    /// Creates a new [`BlockingClient`] using standard blocking I/O types.
    ///
    /// This constructor accepts a blocking reader and writer implementing [`std::io::Read`] and
    /// [`std::io::Write`]. Internally, it spawns two threads:
    /// - one thread for reading from the server and emitting [`Event`]s,
    /// - one thread for writing requests to the server.
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - `BlockingClient`: the handle for sending requests.
    /// - [`BlockingEventReceiver`]: a channel for receiving [`Event`]s emitted by the server.
    /// - Two [`JoinHandle`]s: one for the read thread and one for the write thread. These can be
    ///   used to monitor or explicitly join the background threads if desired.
    ///
    /// [`Event`]: crate::Event
    /// [`BlockingEventReceiver`]: crate::BlockingEventReceiver
    /// [`JoinHandle`]: std::thread::JoinHandle
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
                    .process_incoming(incoming_res?)
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
                let raw_req = write_state.lock().unwrap().track_request(req);
                crate::io::blocking_write(&mut writer, raw_req)?;
            }
            Ok(())
        });
        (Self { tx: req_tx }, event_recv, read_join, write_join)
    }

    /// Sends a single tracked request to the Electrum server and waits for its response.
    ///
    /// This method blocks the current thread until the server replies. It is intended for
    /// request–response RPCs where the response should be handled synchronously.
    ///
    /// # Errors
    ///
    /// Returns [`BlockingRequestError::Dispatch`] if the request could not be sent, or
    /// [`BlockingRequestError::Response`] if the server returned an error. If the request was
    /// canceled or the client shut down, returns [`BlockingRequestError::Canceled`].
    ///
    /// [`BlockingRequestError`]: crate::BlockingRequestError
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

    /// Sends a request that is expected to result in an event-style [`Event`] (such as a
    /// notification).
    ///
    /// This method does not block or wait for a response. Instead, both the initial server response
    /// and any future notifications will be emitted through the [`BlockingEventReceiver`] stream.
    ///
    /// This is useful for subscription-style RPCs like `blockchain.headers.subscribe`, where the
    /// server immediately returns the current state and later sends updates. These can all be
    /// handled as [`Event::Notification`] or [`Event::Response`] values from the receiver.
    ///
    /// # Errors
    ///
    /// Returns [`BlockingRequestSendError`] if the request could not be queued for sending.
    ///
    /// [`Event`]: crate::Event
    /// [`BlockingEventReceiver`]: crate::BlockingEventReceiver
    /// [`BlockingRequestSendError`]: crate::BlockingRequestSendError
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

    /// Sends a batch of requests to the Electrum server.
    ///
    /// The batch is constructed using [`BlockingBatchRequest`], which allows queuing both tracked
    /// requests (via [`BlockingBatchRequest::request`]) and event-style requests (via
    /// [`BlockingBatchRequest::event_request`]).
    ///
    /// Tracked requests return blocking handles that can be used to wait for server responses.
    /// Event-style requests (e.g., subscriptions) still result in a server response, but it is
    /// emitted through the [`BlockingEventReceiver`] instead of through a blocking response handle.
    ///
    /// **Important:** Do not call `.recv()` or `.wait()` on any response handles returned by
    /// [`BlockingBatchRequest::request`] until after the batch has been submitted using
    /// `send_batch`. Doing so will block indefinitely, as the request has not yet been sent.
    ///
    /// # Returns
    /// - `Ok(true)` if the batch was non-empty and sent successfully.
    /// - `Ok(false)` if the batch was empty and nothing was sent.
    /// - `Err` if the batch could not be sent (e.g., if the client was shut down).
    ///
    /// [`BlockingBatchRequest`]: crate::BlockingBatchRequest
    /// [`BlockingBatchRequest::request`]: crate::BlockingBatchRequest::request
    /// [`BlockingBatchRequest::event_request`]: crate::BlockingBatchRequest::event_request
    /// [`BlockingEventReceiver`]: crate::BlockingEventReceiver
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
