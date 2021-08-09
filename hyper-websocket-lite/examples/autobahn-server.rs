use futures_util::{SinkExt, StreamExt};
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use hyper_websocket_lite::{server_upgrade, Client};
use websocket_codec::{Message, Opcode, Result};

async fn on_client(mut stream_mut: Client) {
    let mut stream = loop {
        let (msg, mut stream) = stream_mut.into_future().await;

        let msg = match msg {
            Some(Ok(msg)) => msg,
            Some(Err(_err)) => {
                let _ = stream.send(Message::close(None)).await;
                break stream;
            }
            None => {
                break stream;
            }
        };

        let _ = match msg.opcode() {
            Opcode::Text | Opcode::Binary => stream.send(msg).await,
            Opcode::Ping => stream.send(Message::pong(msg.into_data())).await,
            Opcode::Close => {
                break stream;
            }
            Opcode::Pong => Ok(()),
        };

        stream_mut = stream;
    };

    let _ = stream.send(Message::close(None)).await;
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = ([0, 0, 0, 0], 9001).into();

    let make_service =
        make_service_fn(|_| async { Ok::<_, hyper::Error>(service_fn(|req| server_upgrade(req, on_client))) });

    Server::bind(&addr).serve(make_service).await?;
    Ok(())
}
