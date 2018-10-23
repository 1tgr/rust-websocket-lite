#![deny(warnings)]

extern crate bytes;
extern crate futures;
extern crate tokio;
extern crate url;
extern crate websocket_lite;

#[cfg_attr(not(feature = "nightly"), macro_use)]
extern crate structopt;

use std::io::{self, Read, Write};
use std::mem;
use std::result;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use futures::{Async, Future, IntoFuture, Sink, Stream};
use futures::future::{self, Either, Loop};
use structopt::StructOpt;
use tokio::timer::Delay;
use url::Url;
use websocket_lite::{ClientBuilder, Message, Opcode, Result};

struct Stdin(Bytes);

impl Stdin {
    pub fn new() -> Self {
        Stdin(Bytes::new())
    }
}

impl Stream for Stdin {
    type Item = Bytes;
    type Error = io::Error;

    fn poll(&mut self) -> result::Result<Async<Option<Bytes>>, io::Error> {
        let buffer = mem::replace(&mut self.0, Bytes::new());
        let mut buffer = BytesMut::from(buffer);
        buffer.resize(4096, 0);

        let n = io::stdin().read(&mut *buffer)?;
        buffer.truncate(n);

        if n == 0 {
            Ok(Async::Ready(None))
        } else {
            let buffer = buffer.freeze();
            mem::replace(&mut self.0, buffer.clone());
            Ok(Async::Ready(Some(buffer)))
        }
    }
}

fn parse_secs(s: &str) -> Result<Duration> {
    let n = s.parse()?;
    Ok(Duration::from_secs(n))
}

#[derive(Debug, StructOpt)]
#[structopt(name = "wsdump", about = "WebSocket Simple Dump Tool")]
struct Opt {
    /// wait time(second) after 'EOF' received.
    #[structopt(long = "eof-wait", parse(try_from_str = "parse_secs"), default_value = "0")]
    eof_wait: Duration,

    /// websocket url. ex. ws://echo.websocket.org/
    #[structopt(parse(try_from_str = "Url::parse"))]
    ws_url: Url,
}

fn main() -> Result<()> {
    let Opt { eof_wait, ws_url } = Opt::from_args();

    let f = ClientBuilder::from_url(ws_url)
        .async_connect()
        .and_then(move |client| {
            let (sink, stream) = client.split();

            let send_loop = future::loop_fn((Stdin::new(), sink), move |(stream, sink)| {
                stream
                    .into_future()
                    .map_err(|(e, _stream)| Into::into(e))
                    .and_then(move |(data, stream)| {
                        if let Some(data) = data {
                            Either::A(
                                Message::new(Opcode::Text, data)
                                    .map_err(Into::into)
                                    .into_future()
                                    .and_then(|message| sink.send(message))
                                    .map(|sink| Loop::Continue((stream, sink))),
                            )
                        } else {
                            Either::B(
                                Delay::new(Instant::now() + eof_wait)
                                    .map_err(Into::into)
                                    .and_then(|()| future::ok(Loop::Break(()))),
                            )
                        }
                    })
            });

            let recv_loop = future::loop_fn(stream, |stream| {
                stream
                    .into_future()
                    .map_err(|(e, _stream)| e)
                    .and_then(|(message, client)| {
                        let message = if let Some(message) = message {
                            message
                        } else {
                            return Ok(Loop::Break(()));
                        };

                        let bytes = if let Some(s) = message.as_text() {
                            s.as_bytes()
                        } else {
                            &message.data()
                        };

                        let stdout = io::stdout();
                        let mut stdout = stdout.lock();
                        stdout.write_all(bytes)?;
                        stdout.flush()?;
                        Ok(Loop::Continue(client))
                    })
            });

            Future::select(send_loop, recv_loop)
                .map(|(value, _other)| value)
                .map_err(|(e, _other)| e)
        })
        .map_err(|e| println!("{}", e));

    tokio::run(f);
    Ok(())
}
