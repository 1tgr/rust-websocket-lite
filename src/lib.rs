#![deny(missing_docs)]
#![deny(warnings)]
#![cfg_attr(feature = "nightly", feature(slice_align_to))]
#![cfg_attr(feature = "nightly", feature(test))]
#![cfg_attr(feature = "cargo-clippy", feature(tool_lints))]

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

extern crate base64;
extern crate byteorder;
extern crate bytes;
extern crate futures;
extern crate httparse;
extern crate rand;
extern crate sha1;
extern crate take_mut;
extern crate tokio_codec;
extern crate tokio_io;
extern crate tokio_tcp;
extern crate url;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

#[cfg(all(feature = "nightly", test))]
extern crate test;

#[cfg(feature = "ssl-native-tls")]
extern crate native_tls;

#[cfg(feature = "ssl-native-tls")]
extern crate tokio_tls;

#[cfg(feature = "ssl-openssl")]
extern crate openssl;

#[cfg(feature = "ssl-openssl")]
extern crate tokio_openssl;

mod client;
mod frame;
mod mask;
mod message;
mod opcode;
mod ssl;
mod sync;
mod upgrade;

pub use client::ClientBuilder;
pub use message::{Message, MessageCodec};
pub use opcode::Opcode;

use std::error;
use std::io::{Read, Write};
use std::result;

use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

/// Represents errors that can be exposed by this crate.
pub type Error = Box<error::Error + 'static>;

/// Represents results returned by the non-async functions in this crate.
pub type Result<T> = result::Result<T, Error>;

/// Used by [`AsyncClient`](type.AsyncClient.html) to represent types that are `AsyncRead` and `AsyncWrite`.
pub trait AsyncNetworkStream: AsyncRead + AsyncWrite {}

impl<S> AsyncNetworkStream for S
where
    S: AsyncRead + AsyncWrite,
{
}

/// Used by [`Client`](type.Client.html) to represent types that are `Read` and `Write`.
pub trait NetworkStream: Read + Write {}

impl<S> NetworkStream for S
where
    S: Read + Write,
{
}

/// Exposes a `Sink` and a `Stream` for sending and receiving WebSocket messages asynchronously.
pub type AsyncClient<S> = Framed<S, MessageCodec>;

/// Sends and receives WebSocket messages synchronously.
pub type Client<S> = sync::Framed<S, MessageCodec>;
