//! Holds types that define WebSocket data at a low level.
//!
//! See [RFC6455 "The WebSocket Protocol"](https://tools.ietf.org/html/rfc6455) for a detailed definition of the fields
//! in the frame header and their relation to the overall WebSocket protocol.
pub use crate::frame::{DataLength, FrameHeader, FrameHeaderCodec};
