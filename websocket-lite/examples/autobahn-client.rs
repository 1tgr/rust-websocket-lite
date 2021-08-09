// Based on example code from the rust-websocket project:
// https://github.com/websockets-rs/rust-websocket/blob/0a12e501cba8bb81875c6c9690b57a76955b7beb/examples/autobahn-client.rs
//
// This example code is copyright (c) 2014-2015 Cyderize
#![warn(clippy::pedantic)]
#![allow(clippy::let_underscore_drop)]

use std::io;
use std::io::Write;

use structopt::StructOpt;
use url::Url;
use websocket_lite::{ClientBuilder, Message, Opcode, Result};

#[derive(Debug, StructOpt)]
#[structopt(name = "async-autobahn-client", about = "Client for the Autobahn fuzzing server")]
struct Opt {
    /// websocket url. ex. ws://localhost:9001/
    #[structopt(parse(try_from_str = Url::parse))]
    ws_url: Url,
}

fn main() -> Result<()> {
    let Opt { ws_url } = Opt::from_args();
    let agent = "rust-websocket-lite";
    println!("Using fuzzingserver {}", ws_url);
    println!("Using agent {}", agent);
    println!("Running test suite...");

    let case_count = get_case_count(&ws_url)?;
    for case_id in 1..=case_count {
        let url = format!(
            "{ws_url}runCase?case={case_id}&agent={agent}",
            ws_url = ws_url,
            case_id = case_id,
            agent = agent
        );

        let mut client = ClientBuilder::new(&url)?.connect_insecure()?;

        {
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            write!(stdout, "Executing test case: {}/{}\r", case_id, case_count)?;
            stdout.flush()?;
        }

        while let Ok(Some(message)) = client.receive() {
            match message.opcode() {
                Opcode::Text | Opcode::Binary => client.send(message)?,

                Opcode::Close => {
                    let _ = client.send(Message::close(None));
                    break;
                }

                Opcode::Ping => client.send(Message::pong(message.into_data()))?,

                Opcode::Pong => (),
            }
        }
    }

    update_reports(&ws_url, agent)
}

fn get_case_count(ws_url: &Url) -> Result<usize> {
    let url = format!("{ws_url}getCaseCount", ws_url = ws_url);
    let mut client = ClientBuilder::new(&url)?.connect_insecure()?;
    let mut count = 0;

    while let Some(message) = client.receive()? {
        match message.opcode() {
            Opcode::Text => {
                count = message.as_text().unwrap().parse()?;
                println!("Will run {} cases...", count);
            }

            Opcode::Close => {
                let _ = client.send(Message::close(None));
                break;
            }

            Opcode::Ping => client.send(Message::pong(message.into_data()))?,

            _ => (),
        }
    }

    Ok(count)
}

fn update_reports(ws_url: &Url, agent: &str) -> Result<()> {
    let url = format!("{ws_url}updateReports?agent={agent}", ws_url = ws_url, agent = agent);
    let mut client = ClientBuilder::new(&url)?.connect_insecure()?;
    println!("Updating reports...");

    while let Some(message) = client.receive()? {
        match message.opcode() {
            Opcode::Close => {
                let _ = client.send(Message::close(None));
                break;
            }

            Opcode::Ping => client.send(Message::pong(message.into_data()))?,

            _ => (),
        }
    }

    println!("Reports updated.");
    println!("Test suite finished!");
    Ok(())
}
