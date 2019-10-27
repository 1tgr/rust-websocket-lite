// Based on example code from the rust-websocket project:
// https://github.com/websockets-rs/rust-websocket/blob/0a12e501cba8bb81875c6c9690b57a76955b7beb/examples/async-autobahn-client.rs
//
// This example code is copyright (c) 2014-2015 Cyderize

use std::io::{self, Write};

use tokio::prelude::*;
use websocket_lite::{ClientBuilder, Message, Opcode, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "ws://127.0.0.1:9001";
    let agent = "rust-websocket-lite";

    println!("Using fuzzingserver {}", addr);
    println!("Using agent {}", agent);

    let case_count = get_case_count(addr).await?;
    println!("We will be running {} test cases!", case_count);

    println!("Running test suite...");
    for case_id in 1..(case_count + 1) {
        let url = format!(
            "{addr}/runCase?case={case_id}&agent={agent}",
            addr = addr,
            case_id = case_id,
            agent = agent
        );

        let builder = ClientBuilder::new(&url)?;
        let mut stream_mut = builder.async_connect_insecure().await?;

        {
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            write!(stdout, "Executing test case: {}/{}\r", case_id, case_count)?;
            stdout.flush()?;
        }

        loop {
            let (msg, mut stream) = stream_mut.into_future().await;

            let msg = match msg {
                Some(Ok(msg)) => msg,
                Some(Err(_err)) => {
                    stream.send(Message::close(None)).await?;
                    break;
                }
                None => {
                    break;
                }
            };

            match msg.opcode() {
                Opcode::Text | Opcode::Binary => stream.send(msg).await?,
                Opcode::Ping => stream.send(Message::pong(msg.into_data())).await?,
                Opcode::Close => stream.send(Message::close(None)).await?,
                Opcode::Pong => (),
            }

            stream_mut = stream;
        }
    }

    update_reports(addr, agent).await?;
    println!("Test suite finished!");
    Ok(())
}

async fn get_case_count(addr: &str) -> Result<usize> {
    let url = format!("{}/getCaseCount", addr);
    let builder = ClientBuilder::new(&url)?;
    let s = builder.async_connect_insecure().await?;
    let (msg, _s) = s.into_future().await;
    if let Some(msg) = msg {
        if let Some(text) = msg?.as_text() {
            return Ok(text.parse()?);
        }
    }

    Err("response not recognised".to_owned().into())
}

async fn update_reports(addr: &str, agent: &str) -> Result<()> {
    let url = format!("{addr}/updateReports?agent={agent}", addr = addr, agent = agent);
    println!("Updating reports...");

    let builder = ClientBuilder::new(&url)?;
    let mut sink = builder.async_connect_insecure().await?;
    sink.send(Message::close(None)).await?;
    println!("Reports updated.");
    Ok(())
}
