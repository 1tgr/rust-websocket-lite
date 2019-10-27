#![deny(missing_docs)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![cfg_attr(feature = "nightly", feature(test))]
#![cfg_attr(feature = "cargo-clippy", feature(tool_lints))]

//! A Tokio codec implementation of the WebSocket protocol.
//!
//! This crate does not do any I/O directly. For a full WebSocket client, see the [websocket-lite](https://docs.rs/websocket-lite) crate.

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

#[cfg(all(feature = "nightly", test))]
extern crate test;

mod frame;
mod mask;
mod message;
mod opcode;
mod upgrade;

pub use crate::message::{Message, MessageCodec};
pub use crate::opcode::Opcode;
pub use crate::upgrade::UpgradeCodec;

use std::error;
use std::result;

/// Represents errors that can be exposed by this crate.
pub type Error = Box<dyn error::Error + 'static>;

/// Represents results returned by the non-async functions in this crate.
pub type Result<T> = result::Result<T, Error>;
