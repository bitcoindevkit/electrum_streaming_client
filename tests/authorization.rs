use std::sync::Arc;
use std::time::Duration;

use async_std::net::{TcpListener, TcpStream};
use electrum_streaming_client::{request, AsyncClient, AuthProvider, BatchRequest};
use futures::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use serde_json::{json, Value};

#[test]
fn auth_batch_request() {
    async_std::task::block_on(async {
        async_std::future::timeout(Duration::from_secs(5), async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let stream = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (reader, writer) = stream.split();
            let provider: AuthProvider = Arc::new(|| Some("test-token".into()));
            let (client, _events, worker) = AsyncClient::new(reader, writer, Some(provider));

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
