use std::result;
use std::str::{self, Utf8Error};

use bytes::{BufMut, Bytes, BytesMut};
use tokio_codec::{Decoder, Encoder};

use super::{Error, Opcode, Result};
use super::frame::FrameHeader;
use super::mask::{Mask, Masker};

/// A text string or a block of binary data that can be sent or recevied over a WebSocket.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    opcode: Opcode,
    data: Bytes,
}

impl Message {
    /// Creates a message from a `Bytes` object.
    ///
    /// The message can be tagged as text or binary. When the `is_text` is `true` this function validates the bytes in
    /// `data` and returns `Err` if they do not contain valid UTF-8 text.
    pub fn new<B: Into<Bytes>>(opcode: Opcode, data: B) -> result::Result<Self, Utf8Error> {
        let data = data.into();

        if let Opcode::Text = opcode {
            str::from_utf8(&data)?;
        }

        Ok(Message { opcode, data })
    }

    /// Creates a text message from a `&str`.
    pub fn text(data: &str) -> Self {
        Message {
            opcode: Opcode::Text,
            data: data.into(),
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
    pub fn close(reason: Option<(u16, &str)>) -> Self {
        let data = if let Some((code, reason)) = reason {
            let mut buf = BytesMut::new();
            buf.reserve(2 + reason.len());
            buf.put_u16_be(code);
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
    pub fn ping<B: Into<Bytes>>(data: B) -> Self {
        Message {
            opcode: Opcode::Ping,
            data: data.into(),
        }
    }

    /// Creates a response to a ping message.
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

    /// For text messages, return a reference to the text.
    pub fn as_text(&self) -> Option<&str> {
        if let Opcode::Text = self.opcode {
            Some(unsafe { str::from_utf8_unchecked(&self.data) })
        } else {
            None
        }
    }
}

/// Tokio codec for WebSocket messages.
pub struct MessageCodec {
    masker: Masker,
}

impl MessageCodec {
    pub(crate) fn new() -> Self {
        MessageCodec {
            masker: Masker::new(),
        }
    }
}

fn validate_fragment(opcode: Opcode) -> Result<()> {
    match opcode {
        Opcode::Text | Opcode::Binary => Ok(()),
        _ => Err("control frames must not be fragmented".into()),
    }
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>> {
        let mut state: Option<(Opcode, BytesMut)> = None;
        loop {
            let (header, data_range) = if let Some(tuple) = FrameHeader::validate(&src)? {
                tuple
            } else {
                return Ok(None);
            };

            if data_range.end > src.len() {
                return Ok(None);
            }

            let data = src.split_to(data_range.end)
                .freeze()
                .slice(data_range.start, data_range.end);

            let data = if let Some(mask) = header.mask {
                // Note: clients never need decode masked messages because masking is only used for client -> server frames.
                // However this code is used to test round tripping of masked messages.
                self.masker.mask(data, mask)
            } else {
                data
            };

            state = match (state, header.fin, header.opcode) {
                (Some(_), _, Some(opcode)) => {
                    return Err(format!("continuation frame should have continuation opcode, not {:?}", opcode).into());
                }

                (Some((opcode, mut partial_data)), true, None) => {
                    validate_fragment(opcode)?;
                    partial_data.extend_from_slice(&data);

                    let message = Message::new(opcode, partial_data)?;
                    return Ok(Some(message));
                }

                (Some((opcode, mut partial_data)), false, None) => {
                    validate_fragment(opcode)?;
                    partial_data.extend_from_slice(&data);
                    Some((opcode, partial_data))
                }

                (None, true, Some(opcode)) => {
                    let message = Message::new(opcode, data)?;
                    return Ok(Some(message));
                },

                (None, true, None) => {
                    return Err("first frame should not be continuation".into());
                }

                (None, false, Some(opcode)) => {
                    validate_fragment(opcode)?;
                    Some((opcode, data.into()))
                }

                (None, false, None) => {
                    return Err("received continuation without seeing the first frame".into());
                }
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
        dst.put(self.masker.mask(item.data, mask));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bytes::{BufMut, BytesMut};
    use tokio_codec::{Decoder, Encoder};

    use super::{Message, MessageCodec};
    use frame::FrameHeader;
    use mask::Masker;
    use opcode::Opcode;

    fn round_trips(is_text: bool, data: String) {
        let message = if is_text {
            Message::text(&data)
        } else {
            Message::new(Opcode::Binary, data).unwrap()
        };

        let mut bytes = BytesMut::new();
        MessageCodec::new().encode(message.clone(), &mut bytes).unwrap();

        let mut bytes = BytesMut::from(&bytes[..]);
        let message2 = MessageCodec::new().decode(&mut bytes).unwrap().unwrap();
        assert_eq!(0, bytes.len());
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

        {
            let data = data.as_bytes().into();
            let data = if let Some(mask) = header.mask {
                Masker::new().mask(data, mask)
            } else {
                data
            };

            bytes.put(data);
        }

        let mut bytes = BytesMut::from(&bytes[..]);
        let message2 = MessageCodec::new().decode(&mut bytes).unwrap().unwrap();
        assert_eq!(0, bytes.len());
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
