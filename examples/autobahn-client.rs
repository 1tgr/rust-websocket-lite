// Based on example code from the rust-websocket project:
// https://github.com/websockets-rs/rust-websocket/blob/0a12e501cba8bb81875c6c9690b57a76955b7beb/examples/autobahn-client.rs
//
// This example code is copyright (c) 2014-2015 Cyderize

extern crate websocket_lite;

use std::io::{self, Write};

use websocket_lite::{ClientBuilder, Message, Opcode, Result};

fn main() -> Result<()> {
    let addr = "ws://127.0.0.1:9001";
    let agent = "rust-websocket-lite";
    println!("Using fuzzingserver {}", addr);
    println!("Using agent {}", agent);
    println!("Running test suite...");

    let case_count = get_case_count(addr)?;
    for case_id in 1..(case_count + 1) {
        let url = format!(
            "{addr}/runCase?case={case_id}&agent={agent}",
            addr = addr,
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

        loop {
            let message = match client.receive() {
                Ok(Some(message)) => message,
                Ok(None) | Err(_) => {
                    break;
                }
            };

            match message.opcode() {
                Opcode::Text | Opcode::Binary => client.send(message)?,

                Opcode::Close => {
                    let _ = client.send(Message::close(None));
                    break;
                }

                Opcode::Ping => client.send(Message::pong(message.into_data()))?,

                _ => (),
            }
        }
    }

    update_reports(addr, agent)
}

fn get_case_count(addr: &str) -> Result<usize> {
    let url = format!("{addr}/getCaseCount", addr = addr);
    let mut client = ClientBuilder::new(&url)?.connect_insecure()?;
    let mut count = 0;

    loop {
        let message = if let Some(message) = client.receive()? {
            message
        } else {
            break;
        };

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

fn update_reports(addr: &str, agent: &str) -> Result<()> {
    let url = format!("{addr}/updateReports?agent={agent}", addr = addr, agent = agent);
    let mut client = ClientBuilder::new(&url)?.connect_insecure()?;
    println!("Updating reports...");

    loop {
        let message = if let Some(message) = client.receive()? {
            message
        } else {
            break;
        };

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
