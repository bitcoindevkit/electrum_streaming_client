use std::time::Duration;

use async_std::{net::TcpStream, stream::StreamExt};
use bdk_testenv::{anyhow, bitcoincore_rpc::RpcApi, TestEnv};
use bitcoin::Amount;
use electrum_streaming_client::{
    notification::Notification, request, AsyncClient, Event, SatisfiedRequest,
};
use futures::{
    executor::{block_on, ThreadPool},
    task::SpawnExt,
    AsyncReadExt, FutureExt,
};

#[test]
fn synopsis() -> anyhow::Result<()> {
    let env = TestEnv::new()?;
    let electrum_addr = env.electrsd.electrum_url.clone();
    println!("URL: {}", electrum_addr);

    let wallet_addr = env
        .rpc_client()
        .get_new_address(None, None)?
        .assume_checked();

    let pool = ThreadPool::new()?;
    block_on(async {
        let stream = TcpStream::connect(electrum_addr.as_str()).await?;
        let (read_stream, write_strean) = stream.split();
        let (client, mut event_rx, run_fut) = AsyncClient::new(read_stream, write_strean);
        let run_handle = pool.spawn_with_handle(run_fut)?;

        client.send_event_request(request::HeadersSubscribe)?;
        client.send_event_request(request::ScriptHashSubscribe::from_script(
            wallet_addr.script_pubkey(),
        ))?;
        assert!(matches!(
            event_rx.next().await,
            Some(Event::Response(SatisfiedRequest::HeadersSubscribe { .. }))
        ));
        assert!(matches!(
            event_rx.next().await,
            Some(Event::Response(
                SatisfiedRequest::ScriptHashSubscribe { .. }
            ))
        ));

        const TO_MINE: usize = 3;
        let blockhashes = env.mine_blocks(TO_MINE, Some(wallet_addr.clone()))?;
        println!("MINED: {:?}", blockhashes);
        while let Some(event) = event_rx.next().await {
            if let Event::Notification(Notification::Header(n)) = event {
                if n.height() > TO_MINE as u32 {
                    break;
                }
            }
        }

        assert_eq!(
            client
                .send_request(request::HeaderWithProof {
                    height: 3,
                    cp_height: 3
                })
                .await?
                .header,
            {
                let blockhash = env.rpc_client().get_block_hash(3)?;
                env.rpc_client().get_block_header(&blockhash)?
            },
            "header at height must match"
        );

        println!(
            "HEADERS: {:?}",
            client
                .send_request(request::Headers {
                    start_height: 1,
                    count: 2,
                })
                .await?
        );

        // Make unconfirmed balance.
        env.mine_blocks(101, Some(wallet_addr.clone()))?; // create spendable balance
        let txid = env.rpc_client().send_to_address(
            &wallet_addr,
            Amount::from_btc(1.0).unwrap(),
            None,
            None,
            None,
            None,
            None,
            None,
        )?;
        env.wait_until_electrum_sees_txid(txid, Duration::from_secs(10))?;

        let tx_resp = client.send_request(request::GetTx { txid }).await?;
        println!("GOT TX: {:?}", tx_resp);
        println!(
            "BROADCAST RESULT: {}",
            client
                .send_request(request::BroadcastTx(tx_resp.tx))
                .await?
        );

        println!(
            "GET BALANCE RESP: {:?}",
            client
                .send_request(request::GetBalance::from_script(
                    wallet_addr.script_pubkey(),
                ))
                .await?
        );

        let history_resp = client
            .send_request(request::GetHistory::from_script(
                wallet_addr.script_pubkey(),
            ))
            .await?;
        println!(
            "GET HISTORY RESP: first = {:?} last = {:?}",
            history_resp.first().unwrap(),
            history_resp.last().unwrap()
        );

        let block_hash = env.mine_blocks(1, None)?.first().copied().unwrap();
        let block_height = env.rpc_client().get_block_info(&block_hash)?.height as u32;
        env.wait_until_electrum_sees_block(Duration::from_secs(5))?;

        let tx_merkle = client
            .send_request(request::GetTxMerkle {
                txid,
                height: block_height,
            })
            .await?;
        println!("GET MERKLE: {:?}", tx_merkle);

        let from_pos = client
            .send_request(request::GetTxidFromPos {
                height: block_height,
                tx_pos: tx_merkle.pos,
            })
            .await?;
        println!("TXID FROM POS: {}", from_pos.txid);
        assert_eq!(txid, from_pos.txid);

        // NOTE: This does not work with `electrs`
        // let mempool_history = request::GetMempool::from_script(addr.script_pubkey())
        //     .send(&req_tx)?
        //     .await??;
        // println!("GET MEMPOOL RESP: {:?}", mempool_history);

        let utxos = client
            .send_request(request::ListUnspent::from_script(
                wallet_addr.script_pubkey(),
            ))
            .await?;
        println!(
            "GET UTXOs: first = {:?} last = {:?}",
            utxos.first().unwrap(),
            utxos.last().unwrap()
        );

        // NOTE: This does not work with our version of `electrs`
        // let unsub_resp = request::ScriptHashUnsubscribe::from_script(addr.script_pubkey())
        //     .send(&req_tx)?
        //     .await??;
        // println!("UNSUB RESP: {:?}", unsub_resp);

        let fee_histogram = client.send_request(request::GetFeeHistogram).await?;
        println!("FEE HISTOGRAM: {:?}", fee_histogram);

        let server_banner = client.send_request(request::Banner).await?;
        println!("SERVER BANNER: {}", server_banner);

        client.send_request(request::Ping).await?;
        println!("PING SUCCESS!");

        // // NOTE: Batching does not work until https://github.com/Blockstream/electrs/pull/108 is
        // // merged.
        //
        // let txid1 = env.send(&wallet_addr, Amount::from_btc(0.1)?)?;
        // let txid2 = env.send(&wallet_addr, Amount::from_btc(0.1)?)?;
        // env.mine_blocks(1, None)?;
        // env.wait_until_electrum_sees_txid(txid1, Duration::from_secs(10))?;
        // env.wait_until_electrum_sees_txid(txid2, Duration::from_secs(10))?;
        // let mut batch = client.batch();
        // let tx1_fut = batch.request(request::GetTx(txid1));
        // let tx2_fut = batch.request(request::GetTx(txid2));
        // batch.send()?;
        // let (tx1_res, tx2_res) = futures::join!(tx1_fut, tx2_fut);
        // let (tx1, tx2) = (tx1_res?, tx2_res?);
        // println!("Got tx1: {:?}", tx1);
        // println!("Got tx2: {:?}", tx2);

        // read remaining events.
        while let Some(event) = event_rx.next().now_or_never() {
            println!("EVENT: {:?}", event);
        }

        drop(client);
        run_handle.await?;
        Ok(())
    })
}
