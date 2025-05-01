use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{MaybeBatch, RawNotificationOrResponse, RawRequest};

#[derive(Debug)]
pub struct ReadStreamer<R> {
    reader: Option<R>,
    buf: Vec<u8>,
    queue: VecDeque<RawNotificationOrResponse>,
    err: Option<std::io::Error>,
}

impl<R> ReadStreamer<R> {
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

pub fn blocking_write<W, T>(mut writer: W, msg: T) -> std::io::Result<()>
where
    T: Into<MaybeBatch<RawRequest>>,
    W: std::io::Write,
{
    let mut b = serde_json::to_vec(&msg.into()).expect("must serialize");
    b.push(b'\n');
    writer.write_all(&b)
}

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
