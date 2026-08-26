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
# #[cfg(all(feature = "tokio", feature = "ssl"))]
# mod example {
use electrum_streaming_client::{request, AsyncClient, ConnectConfig};
use futures::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (client, mut events, worker) = AsyncClient::connect(
        "ssl://electrum.blockstream.info:50002",
        &ConnectConfig::default(),
    )
    .await?;
    let worker = tokio::spawn(worker);

    let relay_fee = client.send_request(request::RelayFee).await?;
    println!("Relay fee: {relay_fee:?}");

    client.send_event_request(request::HeadersSubscribe)?;
    println!("Event: {:?}", events.next().await);

    drop(client);
    worker.await??;

    Ok(())
}
# }
```

## Optional Features

- `tokio` (default): Enables Tokio transport support.
- `ssl`: Enables TLS via rustls. Async TLS additionally requires `tokio`.

## License

MIT
