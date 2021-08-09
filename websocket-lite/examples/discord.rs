use futures_util::sink::SinkExt;
use futures_util::StreamExt;
use websocket_lite::{Message, Opcode, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let builder = websocket_lite::ClientBuilder::new("wss://gateway.discord.gg/?v=9&encoding=json")?;
    let mut ws_stream = builder.connect().await?;

    let identify_payload = format!(
        "{{\"op\": 2, \"d\": {{\"token\": \"{}\", \"intents\": 32509, \"properties\": {{\"$os\": \"linux\", \"$browser\": \"rust-websocket-lite\", \"$device\": \"rust-websocket-lite\"}}}}}}",
        std::env::var("DISCORD_TOKEN").unwrap(),
    );
    ws_stream.send(Message::text(identify_payload)).await?;

    while let Some(msg) = ws_stream.next().await {
        if let Ok(m) = msg {
            match m.opcode() {
                Opcode::Text => {
                    println!("{}", m.as_text().unwrap());
                }
                Opcode::Ping => ws_stream.send(Message::pong(m.into_data())).await?,
                Opcode::Close => {
                    let _ = ws_stream.send(Message::close(None)).await;
                    break;
                }
                Opcode::Pong | Opcode::Binary => {}
            }
        } else {
            let _ = ws_stream.send(Message::close(None)).await;
            break;
        }
    }

    Ok(())
}
