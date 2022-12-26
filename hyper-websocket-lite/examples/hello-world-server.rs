#![deny(rust_2018_idioms)]

use std::env;

use futures_util::SinkExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use hyper_websocket_lite::{server_upgrade, AsyncClient};
use websocket_codec::{Message, Result};

async fn on_client(mut client: AsyncClient) {
    let _ = client.send(Message::text("Hello, world!")).await;
    let _ = client.send(Message::close(None)).await;
}

#[tokio::main]
async fn main() -> Result<()> {
    let port = env::args().nth(1).unwrap_or_else(|| "9001".to_owned()).parse()?;
    let addr = ([0, 0, 0, 0], port).into();

    let make_service =
        make_service_fn(|_| async { Ok::<_, hyper::Error>(service_fn(|req| server_upgrade(req, on_client))) });

    Server::bind(&addr).serve(make_service).await?;
    Ok(())
}
