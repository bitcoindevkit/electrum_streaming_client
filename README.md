# electrum_streaming_client

A streaming, sans-IO Electrum client for asynchronous and blocking Rust applications.

This crate provides low-level primitives and high-level clients for communicating with Electrum
servers over JSON-RPC. It supports both asynchronous (`futures`/`tokio`) and blocking transport
models.

## Features

- **Streaming protocol support**: Handles both server-initiated notifications and responses.
- **Transport agnostic**: Works with any I/O type implementing the appropriate `Read`/`Write` traits.
- **Sans-IO core**: The [`RequestTracker`] struct tracks pending requests and handles incoming server messages.
- **Typed request/response system**: Strongly typed Electrum method wrappers with minimal overhead.

## Example (async with Tokio)

```rust,no_run
use electrum_streaming_client::{AsyncClient, Event};
use tokio::net::TcpStream;
use futures::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("127.0.0.1:50001").await?;
    let (reader, writer) = stream.into_split();
    let (client, mut events, worker) = AsyncClient::new_tokio(reader, writer);

    tokio::spawn(worker); // spawn the client worker task

    let mempool_info = client
        .send_request(electrum_streaming_client::request::GetMempoolInfo)
        .await?;
    println!("Mempool info: {mempool_info:?}");

    while let Some(event) = events.next().await {
        println!("Event: {event:?}");
    }

    Ok(())
}
```

## Optional Features

- `tokio`: Enables [`AsyncClient::new_tokio`] for use with Tokio-compatible streams.

## License

MIT

