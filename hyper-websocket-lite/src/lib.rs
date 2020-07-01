#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

//! A WebSocket server implementation on hyper and websocket-lite.

use std::future::Future;

use hyper::header::{self, HeaderValue};
use hyper::upgrade::Upgraded;
use hyper::{Body, Request, Response, StatusCode};
use tokio::task;
use tokio_util::codec::{Decoder, Framed};
use websocket_codec::{ClientRequest, MessageCodec};

pub use websocket_codec::Result;

/// Exposes a `Sink` and a `Stream` for sending and receiving WebSocket messages asynchronously.
pub type AsyncClient = Framed<Upgraded, MessageCodec>;

/// Accepts a client's WebSocket Upgrade request.
pub async fn server_upgrade<OnClient, F>(req: Request<Body>, on_client: OnClient) -> Result<Response<Body>>
where
    OnClient: FnOnce(AsyncClient) -> F + Send + 'static,
    F: Future<Output = ()> + Send,
{
    let mut res = Response::new(Body::empty());

    let ws_accept = if let Ok(req) = ClientRequest::parse(|name| {
        let h = req.headers().get(name)?;
        h.to_str().ok()
    }) {
        req.ws_accept()
    } else {
        *res.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(res);
    };

    task::spawn(async move {
        match req.into_body().on_upgrade().await {
            Ok(upgraded) => {
                let client = MessageCodec::server().framed(upgraded);
                on_client(client).await;
            }
            Err(e) => eprintln!("upgrade error: {}", e),
        }
    });

    *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;

    let headers = res.headers_mut();
    headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    headers.insert(header::SEC_WEBSOCKET_ACCEPT, HeaderValue::from_str(&ws_accept).unwrap());
    Ok(res)
}
