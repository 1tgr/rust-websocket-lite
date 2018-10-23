# websocket-lite

[Documentation](https://docs.rs/websocket-lite)

A fast, low-overhead WebSocket client.

This library is optimised for receiving a high volume of messages over a long period. A key feature is that is makes
no memory allocations once the connection is set up and the initial messages have been sent and received; it reuses
a single pair of buffers, which are sized for the longest message seen so far.

Only asynchronous access is provided at present. `native_tls` provides the TLS functionality for `wss://...` servers.

This crate is fully conformant with the [Autobahn test suite](https://github.com/crossbario/autobahn-testsuite) fuzzingserver module.