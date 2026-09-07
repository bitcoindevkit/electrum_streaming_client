//! Split-capable blocking TLS stream built on rustls.
//!
//! `rustls::StreamOwned` cannot provide independent read and write halves. This adapter shares
//! the rustls connection state but releases its lock before blocking on socket I/O, allowing
//! reads and writes to progress independently.
//!
//! Only the write half sends TLS records, keeping reads independent of blocked writes.
//! KeyUpdate responses are deferred to the next write or flush.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};

const CIPHERTEXT_CHUNK: usize = 16 * 1024;

#[derive(Debug)]
struct TlsOutput<W> {
    /// Destination for queued TLS ciphertext.
    writer: W,
    /// TLS ciphertext from rustls before sending it to `writer`.
    /// `offset` marks the already-written prefix.
    pending_ciphertext: Vec<u8>,
    /// Number of leading bytes in `pending_ciphertext` already accepted by `writer`.
    offset: usize,
}

impl<W: Write> TlsOutput<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            pending_ciphertext: Vec::new(),
            offset: 0,
        }
    }

    /// Queues new ciphertext and writes all pending ciphertext.
    fn queue_and_write_ciphertext(&mut self, ciphertext: &[u8]) -> io::Result<()> {
        self.pending_ciphertext.extend_from_slice(ciphertext);
        while self.offset < self.pending_ciphertext.len() {
            match self.writer.write(&self.pending_ciphertext[self.offset..]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(n) => self.offset += n,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            }
        }
        self.pending_ciphertext.clear();
        self.offset = 0;
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.queue_and_write_ciphertext(&[])?;
        self.writer.flush()
    }
}

#[derive(Debug)]
struct Shared {
    /// TLS protocol state shared by the read and write halves.
    tls_connection: Mutex<rustls::ClientConnection>,
    /// Extra handle used by either half's `Drop` to interrupt blocking socket I/O.
    shutdown_socket: TcpStream,
}

impl Shared {
    fn tls_connection(&self) -> io::Result<MutexGuard<'_, rustls::ClientConnection>> {
        self.tls_connection.lock().map_err(|_poison| {
            io::Error::new(io::ErrorKind::Other, "TLS connection state mutex poisoned")
        })
    }
}

/// Blocking TLS stream over a TCP socket.
///
/// Use [`into_split`](Self::into_split) to give the read and write threads independent halves.
/// A successful write may still have ciphertext queued; use [`Write::flush`] to report
/// pending socket errors.
#[derive(Debug)]
pub struct TlsStream {
    reader: TlsReadHalf,
    writer: TlsWriteHalf,
}

/// Read half of a split [`TlsStream`].
///
/// Dropping this half shuts down the TCP socket so the writer unblocks.
#[derive(Debug)]
pub struct TlsReadHalf {
    shared: Arc<Shared>,
    read_socket: TcpStream,
    /// Inbound TCP bytes not yet consumed by `read_tls`. Incomplete records live in rustls.
    pending_incoming: Vec<u8>,
}

/// Write half of a split [`TlsStream`].
///
/// Dropping this half shuts down the TCP socket so the reader unblocks.
/// A successful write may still have ciphertext queued; use [`Write::flush`] to report
/// pending socket errors.
#[derive(Debug)]
pub struct TlsWriteHalf {
    shared: Arc<Shared>,
    /// Retains and sends TLS ciphertext produced by the shared connection.
    tls_output: TlsOutput<TcpStream>,
}

impl TlsStream {
    /// `conn` must already have completed the handshake.
    pub(super) fn new(conn: rustls::ClientConnection, tcp: TcpStream) -> io::Result<Self> {
        let read_socket = tcp.try_clone()?;
        let write_socket = tcp.try_clone()?;
        let shared = Arc::new(Shared {
            tls_connection: Mutex::new(conn),
            shutdown_socket: tcp,
        });
        Ok(Self {
            reader: TlsReadHalf {
                shared: Arc::clone(&shared),
                read_socket,
                pending_incoming: Vec::new(),
            },
            writer: TlsWriteHalf {
                shared,
                tls_output: TlsOutput::new(write_socket),
            },
        })
    }

    /// Splits this stream into independent read and write halves.
    pub fn into_split(self) -> (TlsReadHalf, TlsWriteHalf) {
        (self.reader, self.writer)
    }
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl Read for TlsReadHalf {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(n) = self.try_read_plaintext(buf)? {
                return Ok(n);
            }
            if self.pending_incoming.is_empty() && !self.read_ciphertext()? {
                self.process_pending_incoming()?;
                return match self.try_read_plaintext(buf)? {
                    Some(n) => Ok(n),
                    None => Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                };
            }
            self.process_pending_incoming()?;
        }
    }
}

impl TlsReadHalf {
    /// Reads buffered plaintext without touching the socket.
    /// `None` needs more ciphertext; `Some(0)` is clean TLS EOF.
    fn try_read_plaintext(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        let mut conn = self.shared.tls_connection()?;
        match conn.reader().read(buf) {
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            other => other.map(Some),
        }
    }

    /// Reads TLS ciphertext into `pending_incoming`; returns `false` on TCP EOF.
    fn read_ciphertext(&mut self) -> io::Result<bool> {
        let mut buf = [0u8; CIPHERTEXT_CHUNK];
        loop {
            match self.read_socket.read(&mut buf) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.pending_incoming.extend_from_slice(&buf[..n]);
                    return Ok(true);
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            }
        }
    }

    /// Feeds `pending_incoming` (or TCP EOF) into rustls.
    fn process_pending_incoming(&mut self) -> io::Result<()> {
        let mut conn = self.shared.tls_connection()?;
        let n = {
            let mut input = self.pending_incoming.as_slice();
            conn.read_tls(&mut input)?
        };
        if n == 0 {
            self.pending_incoming.clear();
        } else {
            self.pending_incoming.drain(..n);
        }
        conn.process_new_packets()
            .map(|_| ())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}

impl Drop for TlsReadHalf {
    fn drop(&mut self) {
        let _ = self.shared.shutdown_socket.shutdown(Shutdown::Both);
    }
}

impl Write for TlsWriteHalf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let prior_ciphertext = {
            let mut conn = self.shared.tls_connection()?;
            let mut ciphertext = Vec::new();
            drain_tls(&mut conn, &mut ciphertext)?;
            ciphertext
        };
        self.tls_output
            .queue_and_write_ciphertext(&prior_ciphertext)?;

        let (ciphertext, n) = {
            let mut conn = self.shared.tls_connection()?;
            let mut ciphertext = Vec::new();
            let n = conn.writer().write(buf)?;
            drain_tls(&mut conn, &mut ciphertext)?;
            (ciphertext, n)
        };
        // Plaintext accepted by rustls must be reported as written.
        match self.tls_output.queue_and_write_ciphertext(&ciphertext) {
            Ok(()) => Ok(n),
            Err(_err) if n > 0 => Ok(n),
            Err(err) => Err(err),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let ciphertext = {
            let mut conn = self.shared.tls_connection()?;
            let mut ciphertext = Vec::new();
            drain_tls(&mut conn, &mut ciphertext)?;
            ciphertext
        };
        self.tls_output.queue_and_write_ciphertext(&ciphertext)?;
        self.tls_output.flush()
    }
}

impl Drop for TlsWriteHalf {
    fn drop(&mut self) {
        let _ = self.shared.shutdown_socket.shutdown(Shutdown::Both);
    }
}

/// Drains queued ciphertext so it can be written after releasing TLS state.
fn drain_tls(conn: &mut rustls::ClientConnection, out: &mut Vec<u8>) -> io::Result<()> {
    // Expose pending KeyUpdate responses to `write_tls`, even during a flush without application data.
    let _ = conn.writer().write(&[])?;
    while conn.wants_write() {
        if conn.write_tls(out)? == 0 {
            break;
        }
    }
    Ok(())
}
