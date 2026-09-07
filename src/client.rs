use std::{future::Future, io, net::Shutdown, net::TcpStream, thread::JoinHandle, time::Duration};

use crate::pending_request::{PendingRequest, RequestExt};
use crate::transport::ConnectTarget;
use crate::*;

// --- Async client type aliases ---

/// The sending half of the channel used to enqueue one or more requests from [`AsyncClient`].
///
/// These requests are processed and forwarded to [`RequestTracker::track_request`] to be assigned an ID and serialized.
pub type AsyncRequestSender = futures::channel::mpsc::UnboundedSender<RawOneOrMany<PendingRequest>>;

/// The error returned by [`AsyncClient::send_request`] when a request fails.
///
/// This may occur if the server responds with an error, the request is canceled, or the client is shut down.
pub type AsyncRequestError = crate::request::Error<AsyncRequestSendError>;

/// The error that occurs when a request cannot be sent into the async request channel.
///
/// This typically means the client's background task has shut down or the queue is disconnected.
pub type AsyncRequestSendError = futures::channel::mpsc::TrySendError<RawOneOrMany<PendingRequest>>;

/// The receiving half of the internal event stream, returned to users of [`AsyncClient`].
///
/// This yields all incoming [`Event`]s from the Electrum server, including notifications and responses.
pub type AsyncEventReceiver = futures::channel::mpsc::UnboundedReceiver<Event>;

// --- Blocking client type aliases ---

/// Channel sender for sending blocking requests from [`BlockingClient`] to the write thread.
pub type BlockingRequestSender = std::sync::mpsc::Sender<RawOneOrMany<PendingRequest>>;

/// Error returned by [`BlockingClient::send_request`] if the request fails or is canceled.
pub type BlockingRequestError = crate::request::Error<BlockingRequestSendError>;

/// Error that occurs when a blocking request cannot be sent to the internal request channel.
///
/// Typically indicates that the client has been shut down.
pub type BlockingRequestSendError = std::sync::mpsc::SendError<RawOneOrMany<PendingRequest>>;

/// Channel receiver used to receive [`Event`]s from the Electrum server.
pub type BlockingEventReceiver = std::sync::mpsc::Receiver<Event>;

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
/// [`AsyncEventReceiver`]: crate::client::AsyncEventReceiver
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
    /// [`AsyncEventReceiver`]: crate::client::AsyncEventReceiver
    /// [`Event`]: crate::Event
    pub fn new<R, W>(
        reader: R,
        mut writer: W,
    ) -> (
        Self,
        AsyncEventReceiver,
        impl Future<Output = io::Result<()>> + Send,
    )
    where
        R: futures::AsyncRead + Send + Unpin,
        W: futures::AsyncWrite + Send + Unpin,
    {
        use futures::{channel::mpsc, StreamExt};
        let (event_tx, event_recv) = mpsc::unbounded::<Event>();
        let (req_tx, mut req_recv) = mpsc::unbounded::<RawOneOrMany<PendingRequest>>();

        let mut incoming_stream =
            crate::io::ReadStreamer::new(futures::io::BufReader::new(reader)).fuse();
        let mut state = RequestTracker::new();
        let mut next_id = 0_u32;

        let fut = async move {
            loop {
                futures::select! {
                    req_opt = req_recv.next() => match req_opt {
                        Some(req) => {
                            let raw_req = state.track_request(&mut next_id, req);
                            crate::io::async_write(&mut writer, raw_req).await?;
                        },
                        None => break,
                    },
                    incoming_opt = incoming_stream.next() => match incoming_opt {
                        Some(incoming_res) => {
                            let event_opt = state
                                .handle_incoming(incoming_res?)
                                .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
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
            io::Result::<()>::Ok(())
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
    /// [`AsyncEventReceiver`]: crate::client::AsyncEventReceiver
    /// [`Event`]: crate::Event
    /// [`AsyncClient::new`]: crate::AsyncClient::new
    #[cfg(feature = "tokio")]
    pub fn new_tokio<R, W>(
        reader: R,
        writer: W,
    ) -> (
        Self,
        AsyncEventReceiver,
        impl Future<Output = io::Result<()>> + Send,
    )
    where
        R: tokio::io::AsyncRead + Send + Unpin,
        W: tokio::io::AsyncWrite + Send + Unpin,
    {
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
        Self::new(reader.compat(), writer.compat_write())
    }

    /// Close the channel.
    pub fn close(&self) {
        self.tx.close_channel();
    }

    /// Creates a new [`AsyncClient`] connected to `addr` over plaintext TCP via Tokio.
    #[cfg(feature = "tokio")]
    pub async fn connect_tcp(
        addr: &crate::transport::ServerAddr,
        timeout: Option<Duration>,
    ) -> io::Result<(
        Self,
        AsyncEventReceiver,
        impl Future<Output = io::Result<()>> + Send,
    )> {
        let stream = crate::transport::tokio::connect_tcp(addr, timeout).await?;
        let (reader, writer) = tokio::io::split(stream);
        Ok(Self::new_tokio(reader, writer))
    }

    /// Creates a new [`AsyncClient`] connected to `addr` over TLS via Tokio.
    ///
    /// `timeout` bounds DNS, TCP connect, and the TLS handshake.
    /// `validate_domain` requires a domain host.
    #[cfg(all(feature = "ssl", feature = "tokio"))]
    pub async fn connect_ssl(
        addr: &crate::transport::ServerAddr,
        validate_domain: bool,
        timeout: Option<Duration>,
    ) -> Result<
        (
            Self,
            AsyncEventReceiver,
            impl Future<Output = io::Result<()>> + Send,
        ),
        crate::ConnectError,
    > {
        let stream = crate::transport::tokio::connect_ssl(addr, validate_domain, timeout).await?;
        let (reader, writer) = tokio::io::split(stream);
        Ok(Self::new_tokio(reader, writer))
    }

    /// Connects to `url` using its scheme.
    ///
    /// Accepts `host:port`, `tcp://host:port`, or `ssl://host:port`.
    /// A missing scheme defaults to plaintext TCP; `ssl://` requires the `ssl` feature.
    #[cfg(feature = "tokio")]
    pub async fn connect(
        url: &str,
        config: &crate::transport::ConnectConfig,
    ) -> Result<
        (
            Self,
            AsyncEventReceiver,
            impl Future<Output = io::Result<()>> + Send,
        ),
        crate::ConnectError,
    > {
        fn box_worker<F>(
            worker: F,
        ) -> std::pin::Pin<Box<dyn Future<Output = io::Result<()>> + Send>>
        where
            F: Future<Output = io::Result<()>> + Send + 'static,
        {
            Box::pin(worker)
        }

        match url.parse::<ConnectTarget>()? {
            ConnectTarget::Tcp(addr) => {
                let (client, events, worker) = Self::connect_tcp(&addr, config.timeout()).await?;
                Ok((client, events, box_worker(worker)))
            }
            #[cfg(feature = "ssl")]
            ConnectTarget::Ssl(addr) => {
                let (client, events, worker) =
                    Self::connect_ssl(&addr, config.validate_domain(), config.timeout()).await?;
                Ok((client, events, box_worker(worker)))
            }
            #[cfg(not(feature = "ssl"))]
            ConnectTarget::Ssl(_) => Err(crate::ConnectError::SslUnsupported),
        }
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
        Req: RequestExt + Send + Sync + 'static,
        Req::Response: Send,
    {
        let mut batch = BatchRequest::new();
        let rx = batch.request_async(req);
        self.send_batch(batch)
            .map_err(AsyncRequestError::Dispatch)?;
        rx.await
            .map_err(|_| AsyncRequestError::Canceled)?
            .map_err(AsyncRequestError::Response)
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
    /// [`AsyncEventReceiver`]: crate::client::AsyncEventReceiver
    /// [`AsyncRequestSendError`]: crate::AsyncRequestSendError
    pub fn send_event_request<Req>(&self, request: Req) -> Result<(), AsyncRequestSendError>
    where
        Req: RequestExt + Send + Sync + 'static,
    {
        let mut batch = BatchRequest::new();
        batch.event_request(request);
        self.send_batch(batch)?;
        Ok(())
    }

    /// Sends a batch of requests to the Electrum server.
    ///
    /// The batch is constructed using [`BatchRequest`], which allows queuing both tracked
    /// requests (via [`BatchRequest::request`]) and event-style requests (via
    /// [`BatchRequest::event_request`]).
    ///
    /// Tracked requests use callbacks that are invoked when the server responds. Event-style
    /// requests (e.g., subscriptions) produce an initial server response delivered through the
    /// [`AsyncEventReceiver`].
    ///
    /// # Returns
    /// - `Ok(true)` if the batch was non-empty and sent successfully.
    /// - `Ok(false)` if the batch was empty and nothing was sent.
    /// - `Err` if the batch could not be sent (e.g., if the client was shut down).
    ///
    /// [`BatchRequest`]: crate::BatchRequest
    /// [`BatchRequest::request`]: crate::BatchRequest::request
    /// [`BatchRequest::event_request`]: crate::BatchRequest::event_request
    /// [`AsyncEventReceiver`]: crate::client::AsyncEventReceiver
    pub fn send_batch(&self, batch_req: BatchRequest) -> Result<bool, AsyncRequestSendError> {
        match batch_req.into_inner() {
            Some(batch) => self.tx.unbounded_send(batch).map(|_| true),
            None => Ok(false),
        }
    }
}

/// A `Write` wrapper around a [`TcpStream`] that shuts down the socket on drop.
///
/// [`BlockingClient::connect_tcp`] reads on a `try_clone` handle while the write thread holds
/// this wrapper. When the last client handle drops, the write thread ends and dropping this
/// wrapper shuts the socket down, unblocking the read thread. A plain `try_clone` handle would
/// otherwise keep the socket alive until the peer closes it.
#[derive(Debug)]
struct ShutdownOnDropTcpWriter(TcpStream);

impl io::Write for ShutdownOnDropTcpWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Drop for ShutdownOnDropTcpWriter {
    fn drop(&mut self) {
        let _ = self.0.shutdown(Shutdown::Both);
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
/// [`BlockingEventReceiver`]: crate::client::BlockingEventReceiver
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
    /// [`BlockingEventReceiver`]: crate::client::BlockingEventReceiver
    /// [`JoinHandle`]: std::thread::JoinHandle
    pub fn new<R, W>(
        reader: R,
        mut writer: W,
    ) -> (
        Self,
        BlockingEventReceiver,
        JoinHandle<io::Result<()>>,
        JoinHandle<io::Result<()>>,
    )
    where
        R: io::Read + Send + 'static,
        W: io::Write + Send + 'static,
    {
        use std::sync::mpsc::*;
        let (event_tx, event_recv) = channel::<Event>();
        let (req_tx, req_recv) = channel::<RawOneOrMany<PendingRequest>>();
        let incoming_stream = crate::io::ReadStreamer::new(io::BufReader::new(reader));
        let read_state = std::sync::Arc::new(std::sync::Mutex::new(RequestTracker::new()));
        let write_state = std::sync::Arc::clone(&read_state);

        let read_join = std::thread::spawn(move || -> io::Result<()> {
            for incoming_res in incoming_stream {
                let event_opt = read_state
                    .lock()
                    .unwrap()
                    .handle_incoming(incoming_res?)
                    .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
                if let Some(event) = event_opt {
                    if let Err(_err) = event_tx.send(event) {
                        break;
                    }
                }
            }
            Ok(())
        });
        let write_join = std::thread::spawn(move || -> io::Result<()> {
            let mut next_id = 0_u32;
            for req in req_recv {
                let raw_req = write_state.lock().unwrap().track_request(&mut next_id, req);
                crate::io::blocking_write(&mut writer, raw_req)?;
            }
            Ok(())
        });
        (Self { tx: req_tx }, event_recv, read_join, write_join)
    }

    /// Connects to `url` using its scheme.
    ///
    /// Accepts `host:port`, `tcp://host:port`, or `ssl://host:port`.
    /// A missing scheme defaults to plaintext TCP; `ssl://` requires the `ssl` feature.
    #[allow(clippy::type_complexity)]
    pub fn connect(
        url: &str,
        config: &crate::transport::ConnectConfig,
    ) -> Result<
        (
            Self,
            BlockingEventReceiver,
            JoinHandle<io::Result<()>>,
            JoinHandle<io::Result<()>>,
        ),
        crate::ConnectError,
    > {
        match url.parse::<ConnectTarget>()? {
            ConnectTarget::Tcp(addr) => {
                Self::connect_tcp(&addr, config.timeout()).map_err(Into::into)
            }
            #[cfg(feature = "ssl")]
            ConnectTarget::Ssl(addr) => {
                Self::connect_ssl(&addr, config.validate_domain(), config.timeout())
            }
            #[cfg(not(feature = "ssl"))]
            ConnectTarget::Ssl(_) => Err(crate::ConnectError::SslUnsupported),
        }
    }

    /// Creates a new [`BlockingClient`] connected to `addr` over plaintext TCP.
    #[allow(clippy::type_complexity)]
    pub fn connect_tcp(
        addr: &crate::transport::ServerAddr,
        timeout: Option<Duration>,
    ) -> io::Result<(
        Self,
        BlockingEventReceiver,
        JoinHandle<io::Result<()>>,
        JoinHandle<io::Result<()>>,
    )> {
        let stream = crate::transport::blocking::connect_tcp(addr, timeout)?;
        let reader = stream.try_clone()?;
        Ok(Self::new(reader, ShutdownOnDropTcpWriter(stream)))
    }

    /// Creates a new [`BlockingClient`] connected to `addr` over TLS.
    ///
    /// `timeout` bounds TCP connect and the TLS handshake.
    /// `validate_domain` requires a domain host.
    #[cfg(feature = "ssl")]
    #[allow(clippy::type_complexity)]
    pub fn connect_ssl(
        addr: &crate::transport::ServerAddr,
        validate_domain: bool,
        timeout: Option<Duration>,
    ) -> Result<
        (
            Self,
            BlockingEventReceiver,
            JoinHandle<io::Result<()>>,
            JoinHandle<io::Result<()>>,
        ),
        crate::ConnectError,
    > {
        let stream = crate::transport::blocking::connect_ssl(addr, validate_domain, timeout)?;
        let (reader, writer) = stream.into_split();
        Ok(Self::new(reader, writer))
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
        Req: RequestExt + Send + Sync + 'static,
        Req::Response: Send,
    {
        let mut batch = BatchRequest::new();
        let rx = batch.request_blocking(req);
        self.send_batch(batch)
            .map_err(BlockingRequestError::Dispatch)?;
        rx.recv()
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
    /// [`BlockingEventReceiver`]: crate::client::BlockingEventReceiver
    /// [`BlockingRequestSendError`]: crate::BlockingRequestSendError
    pub fn send_event_request<Req>(&self, request: Req) -> Result<(), BlockingRequestSendError>
    where
        Req: RequestExt + Send + Sync + 'static,
    {
        let mut batch = BatchRequest::new();
        batch.event_request(request);
        self.send_batch(batch)?;
        Ok(())
    }

    /// Sends a batch of requests to the Electrum server.
    ///
    /// The batch is constructed using [`BatchRequest`], which allows queuing both tracked
    /// requests (via [`BatchRequest::request`]) and event-style requests (via
    /// [`BatchRequest::event_request`]).
    ///
    /// Tracked requests use callbacks that are invoked when the server responds. Event-style
    /// requests (e.g., subscriptions) produce an initial server response delivered through the
    /// [`BlockingEventReceiver`].
    ///
    /// # Returns
    /// - `Ok(true)` if the batch was non-empty and sent successfully.
    /// - `Ok(false)` if the batch was empty and nothing was sent.
    /// - `Err` if the batch could not be sent (e.g., if the client was shut down).
    ///
    /// [`BatchRequest`]: crate::BatchRequest
    /// [`BatchRequest::request`]: crate::BatchRequest::request
    /// [`BatchRequest::event_request`]: crate::BatchRequest::event_request
    /// [`BlockingEventReceiver`]: crate::client::BlockingEventReceiver
    pub fn send_batch(&self, batch_req: BatchRequest) -> Result<bool, BlockingRequestSendError> {
        match batch_req.into_inner() {
            Some(batch) => self.tx.send(batch).map(|_| true),
            None => Ok(false),
        }
    }
}
