//! Low-level I/O utilities for reading and writing Electrum JSON-RPC messages.
//!
//! This module provides core types and functions for serializing and deserializing Electrum
//! messages—including requests, responses, and notifications—over arbitrary transports.
//!
//! [`Decoder`] and [`encode`] do no I/O, so a custom transport only needs to move bytes.

use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{RawIncoming, RawOneOrMany, RawRequest};

/// A sans-io decoder for Electrum's newline-delimited JSON-RPC messages.
///
/// Feed it bytes as they arrive with [`Decoder::feed`], then iterate it for the [`RawIncoming`]s
/// in the complete lines received so far. A batch yields one item per message. A line that isn't
/// valid JSON-RPC yields an error and is skipped.
///
/// ```rust
/// use electrum_streaming_client::io::Decoder;
///
/// let mut decoder = Decoder::new();
/// decoder.feed(b"{\"jsonrpc\":\"2.0\",\"method\":\"blockchain.headers.subscribe\",");
/// assert!(decoder.next().is_none());
/// decoder.feed(b"\"params\":[]}\n");
/// assert!(decoder.next().unwrap().is_ok());
/// ```
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    // Length of the start of `buf` known to hold no newline.
    scanned: usize,
    queue: VecDeque<Result<RawIncoming, serde_json::Error>>,
}

impl Decoder {
    /// Creates an empty decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends bytes received from the server.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Returns true if part of a line is buffered (a message cut off, if the input ended).
    pub fn has_partial_line(&self) -> bool {
        !self.buf.iter().all(u8::is_ascii_whitespace)
    }
}

impl Iterator for Decoder {
    type Item = Result<RawIncoming, serde_json::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.queue.is_empty() {
            // Decode every complete line at once, so the buffer is drained once per feed.
            let Some(i) = self.buf[self.scanned..].iter().rposition(|&b| b == b'\n') else {
                self.scanned = self.buf.len();
                return None;
            };
            let end = self.scanned + i + 1;
            for line in self.buf[..end].split(|&b| b == b'\n') {
                // Skip empty lines, including the empty slice after the final `\n`.
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match serde_json::from_slice::<RawOneOrMany<RawIncoming>>(line) {
                    Ok(msgs) => self.queue.extend(msgs.into_vec().into_iter().map(Ok)),
                    Err(err) => self.queue.push_back(Err(err)),
                }
            }
            self.buf.drain(..end);
            self.scanned = 0;
        }
        self.queue.pop_front()
    }
}

/// A streaming parser for Electrum JSON-RPC messages from an input reader.
///
/// `ReadStreamer` reads from a source implementing [`std::io::BufRead`] or
/// [`futures::io::AsyncBufRead`] (depending on the API used) and decodes it with a [`Decoder`].
///
/// ### Behavior
///
/// - For **blocking transports**, `ReadStreamer` implements [`Iterator`], yielding one
///   [`RawIncoming`] at a time.
/// - For **async transports**, `ReadStreamer` implements [`futures::Stream`], with the same item
///   type.
///
/// It ends when the reader ends or fails. If the reader ends partway through a line, the last item
/// is an [`UnexpectedEof`](std::io::ErrorKind::UnexpectedEof) error.
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
    decoder: Decoder,
}

impl<R> ReadStreamer<R> {
    /// Creates a new `ReadStreamer` with the given reader.
    ///
    /// This does not begin reading immediately; call `.next()` (blocking or async) to start
    /// processing messages.
    pub fn new(reader: R) -> Self {
        Self {
            reader: Some(reader),
            decoder: Decoder::new(),
        }
    }

    fn _end(&mut self, err: Option<std::io::Error>) -> Option<std::io::Result<RawIncoming>> {
        self.reader = None;
        if err.is_none() && !self.decoder.has_partial_line() {
            return None;
        }
        self.decoder = Decoder::new();
        Some(Err(
            err.unwrap_or_else(|| std::io::ErrorKind::UnexpectedEof.into())
        ))
    }
}

impl<R: std::io::BufRead> Iterator for ReadStreamer<R> {
    type Item = std::io::Result<RawIncoming>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.decoder.next() {
                return Some(item.map_err(Into::into));
            }
            let reader = self.reader.as_mut()?;
            match reader.fill_buf() {
                Ok([]) => return self._end(None),
                Ok(bytes) => {
                    let n = bytes.len();
                    self.decoder.feed(bytes);
                    reader.consume(n);
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return self._end(Some(err)),
            }
        }
    }
}

impl<R: futures::AsyncBufRead + Unpin> futures::Stream for ReadStreamer<R> {
    type Item = std::io::Result<RawIncoming>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(item) = this.decoder.next() {
                return Poll::Ready(Some(item.map_err(Into::into)));
            }
            let reader = match this.reader.as_mut() {
                Some(reader) => reader,
                None => return Poll::Ready(None),
            };
            match futures::ready!(Pin::new(&mut *reader).poll_fill_buf(cx)) {
                Ok([]) => return Poll::Ready(this._end(None)),
                Ok(bytes) => {
                    let n = bytes.len();
                    this.decoder.feed(bytes);
                    Pin::new(reader).consume(n);
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Poll::Ready(this._end(Some(err))),
            }
        }
    }
}

/// Encodes a JSON-RPC request or batch as a newline-terminated line, as Electrum's
/// line-delimited JSON-RPC protocol expects.
pub fn encode<T: Into<RawOneOrMany<RawRequest>>>(msg: T) -> Vec<u8> {
    let mut b = serde_json::to_vec(&msg.into()).expect("must serialize");
    b.push(b'\n');
    b
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
    T: Into<RawOneOrMany<RawRequest>>,
    W: std::io::Write,
{
    writer.write_all(&encode(msg))
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
    T: Into<RawOneOrMany<RawRequest>>,
    W: futures::AsyncWrite + Unpin,
{
    use futures::AsyncWriteExt;
    writer.write_all(&encode(msg)).await
}

/// Asynchronously writes a JSON-RPC request or batch to a tokio async writer, followed by a newline.
///
/// This is the `"tokio"` version of [`async_write`].
#[cfg(feature = "tokio")]
pub async fn tokio_write<W, T>(mut writer: W, msg: T) -> std::io::Result<()>
where
    T: Into<RawOneOrMany<RawRequest>>,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    writer.write_all(&encode(msg)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTIFICATION: &[u8] =
        b"{\"jsonrpc\":\"2.0\",\"method\":\"blockchain.headers.subscribe\",\"params\":[]}\n";

    #[test]
    fn decoder_splits_lines_across_feeds() {
        let mut input = NOTIFICATION.to_vec();
        input.extend_from_slice(b"\r\n[{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":5},");
        input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":\"no\"}]\nnot json\n");
        input.extend_from_slice(NOTIFICATION);

        // One byte at a time, and all lines in one feed.
        for chunk_len in [1, input.len()] {
            let mut decoder = Decoder::new();
            let mut out = Vec::new();
            for chunk in input.chunks(chunk_len) {
                decoder.feed(chunk);
                out.extend(&mut decoder);
            }
            assert!(!decoder.has_partial_line());
            assert!(matches!(out[0], Ok(RawIncoming::Notification(_))));
            assert!(matches!(out[1], Ok(RawIncoming::Response(ref r)) if r.id == 1));
            assert!(matches!(out[2], Ok(RawIncoming::Response(ref r)) if r.id == 2));
            assert!(out[3].is_err());
            assert!(matches!(out[4], Ok(RawIncoming::Notification(_))));
            assert_eq!(out.len(), 5);
        }
    }

    #[test]
    fn read_streamer_errors_on_cut_off_line() {
        let input = &NOTIFICATION[..NOTIFICATION.len() - 1];
        let out = ReadStreamer::new(input).collect::<Vec<_>>();
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].as_ref().unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn read_streamer_async_matches_blocking() {
        use futures::StreamExt;
        let mut input = NOTIFICATION.repeat(2);
        input.extend_from_slice(b"{");
        let out = futures::executor::block_on(
            ReadStreamer::new(futures::io::Cursor::new(input)).collect::<Vec<_>>(),
        );
        assert_eq!(out.len(), 3);
        assert!(out[..2].iter().all(|r| r.is_ok()));
        assert!(out[2].is_err());
    }
}
