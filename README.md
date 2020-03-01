# websocket-lite

[![Build Status](https://travis-ci.org/1tgr/rust-websocket-lite.svg?branch=master)](https://travis-ci.org/1tgr/rust-websocket-lite)

This repo contains three crates:
- websocket-lite, a fast, low-overhead WebSocket client 
- websocket-codec, a Tokio codec implementation of the WebSocket protocol
- hyper-websocket-lite, bindings between a [hyper](https://hyper.rs) server and websocket-codec

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

## hyper-websocket-lite

[Documentation](https://docs.rs/hyper-websocket-lite)

Provides the `server_upgrade` function, which bridges a client's HTTP Upgrade request to the WebSocket protocol.

# async/await
Version 0.3.2 and above use `std` futures and the `async` and `await` keywords. They are based on tokio
0.2 and futures 0.3 and the earliest supported compiler is 1.39.

Version 0.2.4 is the release prior to `async`/`await`. It is based on tokio 0.1 and futures 0.1. 

