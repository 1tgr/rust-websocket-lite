# websocket-lite and websocket-codec

This repo contains two crates:
- websocket-lite, a fast, low-overhead WebSocket client 
- websocket-codec, a Tokio codec implementation of the WebSocket protocol

## websocket-lite
[Documentation](https://docs.rs/websocket-lite)

This crate is optimised for receiving a high volume of messages over a long period. A key feature is that it makes
no memory allocations once the connection is set up and the initial messages have been sent and received; it reuses
a single pair of buffers, which are sized for the longest message seen so far.

You can use this crate in both asynchronous (futures-based) and synchronous code.
`native_tls` provides the TLS functionality for `wss://...` servers.

This crate is fully conformant with the fuzzingserver module in the
[Autobahn test suite](https://github.com/crossbario/autobahn-testsuite).

## websocket-codec

[Documentation](https://docs.rs/websocket-codec)

This is a standalone crate that does not do any I/O directly. For a full WebSocket client, see the [websocket-lite](https://docs.rs/websocket-lite) crate.

# `async`/`await`
As of October 2019, the `master` branch builds against Rust nightly, and is expected to build against Rust stable 1.40.
It currently references `futures-preview = "0.3.0-alpha.19"` and `tokio = "0.2.0-alpha.6"`.