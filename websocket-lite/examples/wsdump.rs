use std::io::{self, Write};
use std::time::Duration;

use futures::future::{self, FutureExt};
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use structopt::StructOpt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time;
use url::Url;
use websocket_lite::{ClientBuilder, Message, Opcode, Result};

fn parse_secs(s: &str) -> Result<Duration> {
    let n = s.parse()?;
    Ok(Duration::from_secs(n))
}

#[derive(Debug, StructOpt)]
#[structopt(name = "wsdump", about = "WebSocket Simple Dump Tool")]
struct Opt {
    /// wait time(second) after 'EOF' received.
    #[structopt(long = "eof-wait", parse(try_from_str = parse_secs), default_value = "0")]
    eof_wait: Duration,

    /// websocket url. ex. ws://echo.websocket.org/
    #[structopt(parse(try_from_str = Url::parse))]
    ws_url: Url,
}

#[tokio::main]
async fn main() -> Result<()> {
    let Opt { eof_wait, ws_url } = Opt::from_args();
    let client = ClientBuilder::from_url(ws_url).async_connect().await?;
    let (sink, stream) = client.split();

    let send_loop = async {
        let mut stream_mut = BufReader::new(tokio::io::stdin()).lines();
        let mut sink = sink;

        while let Some(data) = stream_mut.next_line().await? {
            let message = Message::new(Opcode::Text, data)?;
            sink.send(message).await?;
        }

        time::sleep(eof_wait).await;
        Ok(())
    };

    let recv_loop = async {
        let mut stream_mut = stream;

        loop {
            let (message, stream) = stream_mut.into_future().await;

            let message = if let Some(message) = message {
                message?
            } else {
                break;
            };

            if let Opcode::Text | Opcode::Binary = message.opcode() {
                if let Some(s) = message.as_text() {
                    println!("{}", s);
                } else {
                    let stdout = io::stdout();
                    let mut stdout = stdout.lock();
                    stdout.write_all(message.data())?;
                    stdout.flush()?;
                }
            }

            stream_mut = stream;
        }

        Ok(()) as Result<()>
    };

    future::select(send_loop.boxed(), recv_loop.boxed())
        .await
        .into_inner()
        .0?;

    Ok(())
}
