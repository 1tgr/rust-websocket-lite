# websocket-lite

![CI](https://github.com/1tgr/rust-websocket-lite/workflows/CI/badge.svg)

This repo contains three crates:

- websocket-lite, a fast, low-overhead async WebSocket client
- websocket-codec, a Tokio codec implementation of the WebSocket protocol
- hyper-websocket-lite, bindings between a [hyper](https://hyper.rs) server and websocket-codec

## websocket-lite

[Documentation](https://docs.rs/websocket-lite) | [Source](websocket-lite/src)

This crate is optimised for receiving a high volume of messages over a long period. A key feature is that it makes
no memory allocations once the connection is set up and the initial messages have been sent and received; it reuses
a single pair of buffers, which are sized for the longest message seen so far.

This crate provides sync and async, [tokio](https://docs.rs/tokio)-based functionality.
The `ssl-native-tls`, `ssl-rustls-native-roots` and `ssl-rustls-webpki-roots` feature flags provide the TLS functionality for `wss://...` servers.

This crate is fully conformant with the fuzzingserver module in the
[Autobahn test suite](https://github.com/crossbario/autobahn-testsuite).

## websocket-codec

[Documentation](https://docs.rs/websocket-codec) | [Source](websocket-codec/src)

This is a standalone crate that does not do any I/O directly. For a full WebSocket client, see the [websocket-lite](https://docs.rs/websocket-lite) crate.

## hyper-websocket-lite

[Documentation](https://docs.rs/hyper-websocket-lite) | [Source](hyper-websocket-lite/src)

Provides the `server_upgrade` function, which bridges a client's HTTP Upgrade request to the WebSocket protocol.

## Additional command line tools

- [`wsinspect`](websocket-codec/examples/wsinspect.rs): views the protocol-level WebSocket frame data from a binary file.
  ```
  cargo run --example wsinspect -- --help
  ```
- [`wsdump`](websocket-lite/examples/wsdump.rs): a basic replica of the [`wsdump` tool found in the `websocket-client` Python package](https://github.com/websocket-client/websocket-client/blob/master/bin/wsdump.py).
  ```
  cargo run --example wsdump -- --help
  ```

# async/await

Version 0.3.2 and above use `std` futures and the `async` and `await` keywords. They are based on tokio
0.2 and futures 0.3 and the earliest supported compiler is 1.39. Version 0.5.0 and above use tokio 1.x and futures 0.3.

Version 0.2.4 is the release prior to `async`/`await`. It is based on tokio 0.1 and futures 0.1.
