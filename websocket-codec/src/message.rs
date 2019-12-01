use std::mem;
use std::result;
use std::str::{self, Utf8Error};

use bytes::{BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::frame::FrameHeader;
use crate::mask::{self, Mask};
use crate::{Error, Opcode, Result};

/// A text string, a block of binary data or a WebSocket control frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    opcode: Opcode,
    data: Bytes,
}

impl Message {
    /// Creates a message from a `Bytes` object.
    ///
    /// The message can be tagged as text or binary. When the `opcode` parameter is [`Opcode::Text`](enum.Opcode.html)
    /// this function validates the bytes in `data` and returns `Err` if they do not contain valid UTF-8 text.
    pub fn new<B: Into<Bytes>>(opcode: Opcode, data: B) -> result::Result<Self, Utf8Error> {
        let data = data.into();

        if opcode.is_text() {
            str::from_utf8(&data)?;
        }

        Ok(Message { opcode, data })
    }

    /// Creates a text message from a `String`.
    pub fn text<S: Into<String>>(data: S) -> Self {
        Message {
            opcode: Opcode::Text,
            data: data.into().into(),
        }
    }

    /// Creates a binary message from any type that can be converted to `Bytes`, such as `&[u8]` or `Vec<u8>`.
    pub fn binary<B: Into<Bytes>>(data: B) -> Self {
        Message {
            opcode: Opcode::Binary,
            data: data.into(),
        }
    }

    /// Creates a message that indicates the connection is about to be closed.
    ///
    /// The `reason` parameter is an optional numerical status code and text description. Valid reasons
    /// may be defined by a particular WebSocket server.
    pub fn close(reason: Option<(u16, String)>) -> Self {
        let data = if let Some((code, reason)) = reason {
            let reason: Bytes = reason.into();
            let mut buf = BytesMut::new();
            buf.reserve(2 + reason.len());
            buf.put_u16(code);
            buf.put(reason);
            buf.freeze()
        } else {
            Bytes::new()
        };

        Message {
            opcode: Opcode::Close,
            data,
        }
    }

    /// Creates a message requesting a pong response.
    ///
    /// The client can send one of these to request a pong response from the server.
    pub fn ping<B: Into<Bytes>>(data: B) -> Self {
        Message {
            opcode: Opcode::Ping,
            data: data.into(),
        }
    }

    /// Creates a response to a ping message.
    ///
    /// The client can send one of these in response to a ping from the server.
    pub fn pong<B: Into<Bytes>>(data: B) -> Self {
        Message {
            opcode: Opcode::Pong,
            data: data.into(),
        }
    }

    /// Returns this message's WebSocket opcode.
    pub fn opcode(&self) -> Opcode {
        self.opcode
    }

    /// Returns a reference to the data held in this message.
    pub fn data(&self) -> &Bytes {
        &self.data
    }

    /// Consumes the message, returning its data.
    pub fn into_data(self) -> Bytes {
        self.data
    }

    /// For messages with opcode [`Opcode::Text`](enum.Opcode.html), returns a reference to the text.
    /// Returns `None` otherwise.
    pub fn as_text(&self) -> Option<&str> {
        if self.opcode.is_text() {
            Some(unsafe { str::from_utf8_unchecked(&self.data) })
        } else {
            None
        }
    }
}

/// Tokio codec for WebSocket messages. This codec can send and receive [`Message`](struct.Message.html) structs.
pub struct MessageCodec {
    interrupted_message: Option<(Opcode, BytesMut)>,
}

impl MessageCodec {
    /// Creates a `MessageCodec`.
    pub fn new() -> Self {
        MessageCodec {
            interrupted_message: None,
        }
    }
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>> {
        let mut state = mem::replace(&mut self.interrupted_message, None);
        loop {
            let (header, data_range) = if let Some(tuple) = FrameHeader::validate(&src)? {
                tuple
            } else {
                // The buffer isn't big enough for the frame header. Reserve additional space for a frame header,
                // plus reasonable extensions.
                src.reserve(512);
                self.interrupted_message = state;
                return Ok(None);
            };

            if data_range.end > src.len() {
                // The buffer contains the frame header but it's not big enough for the data. Reserve additional
                // space for the frame data, plus the next frame header.
                src.reserve(data_range.end + 512);
                self.interrupted_message = state;
                return Ok(None);
            }

            // The buffer contains the frame header and all of the data. We can parse it and return Ok(Some(...)).

            let mut data = src.split_to(data_range.end).split_off(data_range.start);
            if let Some(mask) = header.mask {
                // Note: clients never need decode masked messages because masking is only used for client -> server frames.
                // However this code is used to test round tripping of masked messages.
                mask::mask_slice(&mut data, mask)
            };
            state = if let Some((partial_opcode, mut partial_data)) = state {
                if let Some(opcode) = header.opcode {
                    if header.fin && opcode.is_control() {
                        self.interrupted_message = Some((partial_opcode, partial_data));
                        return Ok(Some(Message::new(opcode, data.freeze())?));
                    }

                    return Err(format!("continuation frame must have continuation opcode, not {:?}", opcode).into());
                } else {
                    partial_data.extend_from_slice(&data);

                    if header.fin {
                        return Ok(Some(Message::new(partial_opcode, partial_data.freeze())?));
                    }

                    Some((partial_opcode, partial_data))
                }
            } else if let Some(opcode) = header.opcode {
                if header.fin {
                    return Ok(Some(Message::new(opcode, data.freeze())?));
                }
                if opcode.is_control() {
                    return Err("control frames must not be fragmented".into());
                }
                Some((opcode, data))
            } else {
                return Err("continuation must not be first frame".into());
            }
        }
    }
}

impl Encoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<()> {
        let mask = Mask::new();

        let header = FrameHeader {
            fin: true,
            opcode: Some(item.opcode),
            mask: Some(mask),
            len: item.data.len(),
        };

        header.write_to(dst);

        let offset = dst.len();
        dst.resize(offset + item.data.len(), 0);
        mask::mask_slice_copy(&mut dst[offset..], &item.data, mask);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bytes::{BufMut, BytesMut};
    use tokio_util::codec::{Decoder, Encoder};

    use crate::frame::FrameHeader;
    use crate::mask;
    use crate::opcode::Opcode;
    use crate::{Message, MessageCodec};

    fn decode<D: Decoder>(decoder: &mut D, mut src: &[u8]) -> Result<D::Item, D::Error> {
        let mut decoder_buf = BytesMut::new();
        loop {
            if let Some(result) = decoder.decode(&mut decoder_buf)? {
                assert_eq!(0, decoder_buf.len(), "expected decoder to consume the whole buffer");
                return Ok(result);
            }

            let n = decoder_buf.remaining_mut().min(src.len());
            assert!(n > 0, "expected decoder to reserve at least one byte");
            decoder_buf.put_slice(&src[..n]);
            src = &src[n..];
        }
    }

    fn round_trips(is_text: bool, data: String) {
        let message = if is_text {
            Message::text(data)
        } else {
            Message::new(Opcode::Binary, data).unwrap()
        };

        let mut bytes = BytesMut::new();
        MessageCodec::new().encode(message.clone(), &mut bytes).unwrap();

        let message2 = decode(&mut MessageCodec::new(), &mut bytes).unwrap();
        assert_eq!(message, message2);
    }

    fn round_trips_via_frame_header(is_text: bool, mask: Option<u32>, data: String) {
        let header = FrameHeader {
            fin: true, // TODO test messages split across frames
            opcode: Some(if is_text { Opcode::Text } else { Opcode::Binary }),
            mask: mask.map(|n| n.into()),
            len: data.len(),
        };

        let mut bytes = BytesMut::new();
        header.write_to(&mut bytes);

        if let Some(mask) = header.mask {
            let offset = bytes.len();
            bytes.resize(offset + data.len(), 0);
            mask::mask_slice_copy(&mut bytes[offset..], data.as_bytes(), mask);
        } else {
            bytes.put(data.as_bytes());
        }

        let message2 = decode(&mut MessageCodec::new(), &bytes).unwrap();
        assert_eq!(is_text, message2.as_text().is_some());
        assert_eq!(data.as_bytes(), message2.data());
    }

    quickcheck! {
        fn qc_round_trips(is_text: bool, data: String) -> bool {
            round_trips(is_text, data);
            true
        }

        fn qc_round_trips_via_frame_header(is_text: bool, mask: Option<u32>, data: String) -> bool {
            round_trips_via_frame_header(is_text, mask, data);
            true
        }
    }
}
