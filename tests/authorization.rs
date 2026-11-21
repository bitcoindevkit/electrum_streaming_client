use std::time::Duration;

use async_std::net::{TcpListener, TcpStream};
use electrum_streaming_client::{request, AsyncClient, BatchRequest, BlockingClient};
use futures::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, StreamExt};
use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_secs(5);

/// Returns the `authorization` field of a serialized request, if present.
fn authorization(request: &str) -> Option<Value> {
    let request: Value = serde_json::from_str(request).unwrap();
    request.get("authorization").cloned()
}

/// The `authorization` fields expected from the requests sent by the `auth_changes_apply_in_order_*`
/// tests.
fn expected_authorizations() -> [Option<Value>; 4] {
    [None, Some(json!("token-a")), Some(json!("token-b")), None]
}

/// Checks that every request in a batch carries the authorization token.
#[test]
fn auth_batch_request() {
    async_std::task::block_on(async {
        async_std::future::timeout(TIMEOUT, async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let stream = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (reader, writer) = stream.split();
            let (client, _events, worker) = AsyncClient::new(reader, writer);
            client.set_authorization(Some("test-token".into())).unwrap();

            let server = async {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut reader = futures::io::BufReader::new(stream.clone());
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let requests: Vec<Value> = serde_json::from_str(&line).unwrap();

                assert_eq!(requests.len(), 2);
                for request in &requests {
                    assert_eq!(request["authorization"], "test-token");
                }

                let responses = json!([
                    {"jsonrpc": "2.0", "id": requests[1]["id"], "result": "test-server"},
                    {"jsonrpc": "2.0", "id": requests[0]["id"], "result": null}
                ]);
                let response = format!("{responses}\n");
                stream.write_all(response.as_bytes()).await.unwrap();
            };

            let receive = async {
                let mut batch = BatchRequest::new();
                let ping = batch.request_async(request::Ping);
                let banner = batch.request_async(request::Banner);
                assert!(client.send_batch(batch).unwrap());

                ping.await.unwrap().unwrap();
                banner.await.unwrap().unwrap();
                drop(client);
            };

            let (worker_result, (), ()) = futures::join!(worker, server, receive);
            worker_result.unwrap();
        })
        .await
        .expect("batch test timed out");
    });
}

/// Checks that the async worker sets, replaces, and clears authorization in queue order.
#[test]
fn auth_changes_apply_in_order_async() {
    async_std::task::block_on(async {
        async_std::future::timeout(TIMEOUT, async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let stream = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (reader, writer) = stream.split();
            let (client, _events, worker) = AsyncClient::new(reader, writer);

            let server = async {
                let (stream, _) = listener.accept().await.unwrap();
                let authorizations = futures::io::BufReader::new(stream)
                    .lines()
                    .take(expected_authorizations().len())
                    .map(|line| authorization(&line.unwrap()))
                    .collect::<Vec<_>>()
                    .await;

                assert_eq!(authorizations, expected_authorizations());
            };

            // everything is queued before the worker is polled
            client.send_event_request(request::Ping).unwrap();
            client.set_authorization(Some("token-a".into())).unwrap();
            client.send_event_request(request::Ping).unwrap();
            client.set_authorization(Some("token-b".into())).unwrap();
            client.send_event_request(request::Ping).unwrap();
            client.set_authorization(None).unwrap();
            client.send_event_request(request::Ping).unwrap();
            drop(client);

            let (worker_result, ()) = futures::join!(worker, server);
            worker_result.unwrap();
        })
        .await
        .expect("send order test timed out");
    });
}

/// Checks that the blocking worker sets, replaces, and clears authorization in queue order.
#[test]
fn auth_changes_apply_in_order_blocking() {
    use std::io::BufRead;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (client, _events, read_join, write_join) =
        BlockingClient::new(stream.try_clone().unwrap(), stream);

    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        let authorizations = std::io::BufReader::new(stream)
            .lines()
            .take(expected_authorizations().len())
            .map(|line| authorization(&line.unwrap()))
            .collect::<Vec<_>>();

        assert_eq!(authorizations, expected_authorizations());
    });

    client.send_event_request(request::Ping).unwrap();
    client.set_authorization(Some("token-a".into())).unwrap();
    client.send_event_request(request::Ping).unwrap();
    client.set_authorization(Some("token-b".into())).unwrap();
    client.send_event_request(request::Ping).unwrap();
    client.set_authorization(None).unwrap();
    client.send_event_request(request::Ping).unwrap();
    drop(client);

    write_join.join().unwrap().unwrap();
    server.join().unwrap();
    read_join.join().unwrap().unwrap();
}
