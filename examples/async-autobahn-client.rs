// Based on example code from the rust-websocket project:
// https://github.com/websockets-rs/rust-websocket/blob/0a12e501cba8bb81875c6c9690b57a76955b7beb/examples/async-autobahn-client.rs
//
// This example code is copyright (c) 2014-2015 Cyderize

extern crate futures;
extern crate tokio_core;
extern crate websocket_lite;

use std::io::{self, Write};

use futures::{Future, IntoFuture};
use futures::future::{self, Loop};
use futures::sink::Sink;
use futures::stream::Stream;
use tokio_core::reactor::Core;
use websocket_lite::{ClientBuilder, Error, Message, Opcode, Result};

fn main() -> Result<()> {
    let addr = "ws://127.0.0.1:9001";
    let agent = "rust-websocket-lite";
    let mut core = Core::new()?;

    println!("Using fuzzingserver {}", addr);
    println!("Using agent {}", agent);

    let case_count = core.run(get_case_count(addr))?;
    println!("We will be running {} test cases!", case_count);

    println!("Running test suite...");
    for case_id in 1..(case_count + 1) {
        let url = format!(
            "{addr}/runCase?case={case_id}&agent={agent}",
            addr = addr,
            case_id = case_id,
            agent = agent
        );

        let test_case = ClientBuilder::new(&url)
            .map_err(Into::into)
            .into_future()
            .and_then(|builder| builder.async_connect_insecure())
            .and_then(|duplex| {
                let stdout = io::stdout();
                let mut stdout = stdout.lock();
                write!(stdout, "Executing test case: {}/{}\r", case_id, case_count)?;
                stdout.flush()?;
                Ok(duplex)
            })
            .and_then(move |duplex| {
                future::loop_fn(duplex, |stream| {
                    stream
                        .into_future()
                        .or_else(|(_err, stream)| stream.send(Message::close(None)).map(|s| (None, s)))
                        .and_then(|(msg, stream)| -> Box<Future<Item = _, Error = _>> {
                            let msg = if let Some(msg) = msg {
                                msg
                            } else {
                                return Box::new(future::ok(Loop::Break(())));
                            };

                            match msg.opcode() {
                                Opcode::Text | Opcode::Binary => {
                                    Box::new(stream.send(msg).map(|stream| Loop::Continue(stream)))
                                }

                                Opcode::Ping => Box::new(
                                    stream
                                        .send(Message::pong(msg.into_data()))
                                        .map(|stream| Loop::Continue(stream)),
                                ),

                                Opcode::Close => Box::new(stream.send(Message::close(None)).map(|_| Loop::Break(()))),
                                Opcode::Pong => Box::new(future::ok(Loop::Continue(stream))),
                            }
                        })
                })
            });

        if let Err(err) = core.run(test_case) {
            println!("Test case {} ended with an error: {}", case_id, err);
        }
    }

    core.run(update_reports(addr, agent))?;
    println!("Test suite finished!");
    Ok(())
}

fn get_case_count(addr: &str) -> impl Future<Item = usize, Error = Error> {
    let url = format!("{}/getCaseCount", addr);
    ClientBuilder::new(&url)
        .map_err(Into::into)
        .into_future()
        .and_then(|builder| builder.async_connect_insecure())
        .and_then(|s| s.into_future().map_err(|e| e.0))
        .and_then(|(msg, _)| {
            if let Some(msg) = msg {
                if let Some(text) = msg.as_text() {
                    return Ok(text.parse()?);
                }
            }

            Err("response not recognised".to_owned().into())
        })
}

fn update_reports(addr: &str, agent: &str) -> impl Future<Item = (), Error = Error> {
    let url = format!("{addr}/updateReports?agent={agent}", addr = addr, agent = agent);
    future::ok(())
        .and_then(move |()| {
            println!("Updating reports...");
            ClientBuilder::new(&url).map_err(Into::into)
        })
        .and_then(|builder| builder.async_connect_insecure())
        .and_then(|sink| sink.send(Message::close(None)))
        .map(|_| {
            println!("Reports updated.");
        })
}
