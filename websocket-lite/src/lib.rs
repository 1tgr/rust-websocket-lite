#![warn(missing_docs)]
#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![cfg_attr(feature = "nightly", feature(test))]

//! A fast, low-overhead WebSocket client.
//!
//! This crate is optimised for receiving a high volume of messages over a long period. A key feature is that it makes
//! no memory allocations once the connection is set up and the initial messages have been sent and received; it reuses
//! a single pair of buffers, which are sized for the longest message seen so far.
//!
//! You can use this crate in both asynchronous (futures-based) and synchronous code.
//! `native_tls` provides the TLS functionality for `wss://...` servers.
//!
//! This crate is fully conformant with the fuzzingserver module in the
//! [Autobahn test suite](https://github.com/crossbario/autobahn-testsuite).

mod client;
mod ssl;
mod sync;

pub use crate::client::ClientBuilder;
pub use crate::ssl::{Connector, MaybeTlsStream, SyncMaybeTlsStream};

pub use websocket_codec::{CloseCode, CloseFrame, Error, Message, MessageCodec, Opcode, Result};

use tokio_util::codec::Framed;

/// Exposes a `Sink` and a `Stream` for sending and receiving WebSocket messages asynchronously.
pub type AsyncClient<S> = Framed<S, MessageCodec>;

/// Sends and receives WebSocket messages synchronously.
pub type Client<S> = sync::Framed<S, MessageCodec>;
