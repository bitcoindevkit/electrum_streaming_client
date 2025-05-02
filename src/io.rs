//! Low-level I/O utilities for reading and writing Electrum JSON-RPC messages.
//!
//! This module provides core types and functions for serializing and deserializing Electrum
//! messages—including requests, responses, and notifications—over arbitrary transports.

use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{MaybeBatch, RawNotificationOrResponse, RawRequest};

/// A streaming parser for Electrum JSON-RPC messages from an input reader.
///
/// `ReadStreamer` incrementally reads from a source implementing [`std::io::BufRead`] or
/// [`futures::io::AsyncBufRead`] (depending on the API used), parses incoming JSON-RPC payloads, and
/// queues deserialized [`RawNotificationOrResponse`] items for consumption.
///
/// ### Behavior
///
/// - For **blocking transports**, `ReadStreamer` implements [`Iterator`], yielding one
///   [`RawNotificationOrResponse`] at a time.
/// - For **async transports**, `ReadStreamer` implements [`futures::Stream`], with the same item
///   type.
///
/// ### Examples
///
/// **Blocking I/O**
///
/// ```rust
/// use electrum_streaming_client::io::ReadStreamer;
/// use std::io::BufReader;
///
/// let json_lines = b"{\"jsonrpc\":\"2.0\",\"method\":\"blockchain.headers.subscribe\",\"params\":[]}\n";
/// let reader = BufReader::new(&json_lines[..]);
/// let mut streamer = ReadStreamer::new(reader);
///
/// for msg in streamer {
///     println!("Got message: {:?}", msg);
/// }
/// ```
///
/// **Async I/O**
///
/// ```rust
/// use electrum_streaming_client::io::ReadStreamer;
/// use futures::executor::block_on;
/// use futures::stream::StreamExt;
/// use futures::io::Cursor;
///
/// let json_lines = b"{\"jsonrpc\":\"2.0\",\"method\":\"blockchain.headers.subscribe\",\"params\":[]}\n";
/// let reader = Cursor::new(&json_lines[..]);
/// let mut streamer = ReadStreamer::new(reader);
///
/// block_on(async {
///     while let Some(msg) = streamer.next().await {
///         println!("Got message: {:?}", msg);
///     }
/// });
/// ```
#[derive(Debug)]
pub struct ReadStreamer<R> {
    reader: Option<R>,
    buf: Vec<u8>,
    queue: VecDeque<RawNotificationOrResponse>,
    err: Option<std::io::Error>,
}

impl<R> ReadStreamer<R> {
    /// Creates a new `ReadStreamer` with the given reader.
    ///
    /// This does not begin reading immediately; call `.next()` (blocking or async) to start
    /// processing messages.
    pub fn new(reader: R) -> Self {
        Self {
            reader: Some(reader),
            buf: Vec::new(),
            queue: VecDeque::new(),
            err: None,
        }
    }

    fn _enqueue_from_buf(&mut self) -> bool {
        match self.buf.pop() {
            Some(b) => assert_eq!(b, b'\n'),
            None => return false,
        }
        match serde_json::from_slice::<MaybeBatch<RawNotificationOrResponse>>(&self.buf) {
            Ok(MaybeBatch::Single(t)) => self.queue.push_back(t),
            Ok(MaybeBatch::Batch(v)) => self.queue.extend(v),
            Err(err) => {
                self.err = Some(err.into());
                return false;
            }
        };
        self.buf.clear();
        true
    }
}

impl<R: std::io::BufRead> Iterator for ReadStreamer<R> {
    type Item = std::io::Result<RawNotificationOrResponse>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.queue.pop_front() {
                return Some(Ok(item));
            }
            if let Some(err) = self.err.take() {
                return Some(Err(err));
            }
            let mut reader = self.reader.take()?;
            if let Err(err) = reader.read_until(b'\n', &mut self.buf) {
                self.err = Some(err);
                continue;
            };
            if self._enqueue_from_buf() {
                self.reader = Some(reader);
            }
        }
    }
}

impl<R: futures::AsyncBufRead + Unpin> futures::Stream for ReadStreamer<R> {
    type Item = std::io::Result<RawNotificationOrResponse>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use futures::AsyncBufReadExt;
        use futures::FutureExt;
        Poll::Ready(loop {
            if let Some(item) = self.queue.pop_front() {
                break Some(Ok(item));
            }
            if let Some(err) = self.err.take() {
                break Some(Err(err));
            }
            let mut reader = match self.reader.take() {
                Some(r) => r,
                None => break None,
            };
            match reader.read_until(b'\n', &mut self.buf).poll_unpin(cx) {
                Poll::Ready(Err(err)) => {
                    self.err = Some(err);
                    continue;
                }
                Poll::Ready(Ok(_)) => {
                    if self._enqueue_from_buf() {
                        self.reader = Some(reader);
                    }
                }
                Poll::Pending => {
                    self.reader = Some(reader);
                    return Poll::Pending;
                }
            }
        })
    }
}

/// Writes a JSON-RPC request or batch to a blocking writer, followed by a newline.
///
/// The message is serialized using `serde_json` and written as a single line,
/// terminated by `\n`, to comply with Electrum's line-delimited JSON-RPC protocol.
///
/// Returns an error if writing to the underlying writer fails.
///
/// # Parameters
/// - `writer`: A blocking writer implementing [`std::io::Write`].
/// - `msg`: A single or batched [`RawRequest`] to be serialized.
///
/// # Errors
/// Returns a [`std::io::Error`] if the write operation fails.
pub fn blocking_write<W, T>(mut writer: W, msg: T) -> std::io::Result<()>
where
    T: Into<MaybeBatch<RawRequest>>,
    W: std::io::Write,
{
    let mut b = serde_json::to_vec(&msg.into()).expect("must serialize");
    b.push(b'\n');
    writer.write_all(&b)
}

/// Asynchronously writes a JSON-RPC request or batch to an async writer, followed by a newline.
///
/// The message is serialized using `serde_json` and written as a single line terminated by `\n`,
/// following Electrum's line-delimited JSON-RPC protocol.
///
/// # Parameters
/// - `writer`: An async writer implementing [`futures::io::AsyncWrite`] + [`Unpin`].
/// - `msg`: A single or batched [`RawRequest`] to be serialized.
///
/// # Errors
/// Returns a [`std::io::Error`] if the async write operation fails.
pub async fn async_write<W, T>(mut writer: W, msg: T) -> std::io::Result<()>
where
    T: Into<MaybeBatch<RawRequest>>,
    W: futures::AsyncWrite + Unpin,
{
    use futures::AsyncWriteExt;
    let mut b = serde_json::to_vec(&msg.into()).expect("must serialize");
    b.push(b'\n');
    writer.write_all(&b).await
}
