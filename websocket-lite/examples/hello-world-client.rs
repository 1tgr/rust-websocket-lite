#![warn(clippy::pedantic)]
#![allow(clippy::let_underscore_drop)]

use std::env;

use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use websocket_lite::{Message, Opcode, Result};

async fn run() -> Result<()> {
    let url = env::args().nth(1).unwrap_or_else(|| "ws://localhost:9001".to_owned());
    let builder = websocket_lite::ClientBuilder::new(&url)?;
    let mut ws_stream = builder.async_connect().await?;

    loop {
        let msg: Option<Result<Message>> = ws_stream.next().await;

        let msg = if let Some(msg) = msg {
            msg
        } else {
            break;
        };

        let msg = if let Ok(msg) = msg {
            msg
        } else {
            let _ = ws_stream.send(Message::close()).await;
            break;
        };

        match msg.opcode() {
            Opcode::Text => {
                println!("{}", msg.as_text().unwrap());
                ws_stream.send(msg).await?;
            }
            Opcode::Binary => ws_stream.send(msg).await?,
            Opcode::Ping => ws_stream.send(Message::pong(msg.into_data())).await?,
            Opcode::Close => {
                let _ = ws_stream.send(Message::close()).await;
                break;
            }
            Opcode::Pong => {}
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    tokio::spawn(async {
        run().await.unwrap_or_else(|e| {
            eprintln!("{}", e);
        })
    })
    .await
    .unwrap();
}
