use std::result;
use std::str::{self, Utf8Error};

use bytes::{BufMut, Bytes, BytesMut};
use tokio_codec::{Decoder, Encoder};

use super::{Error, Result};
use super::frame::FrameHeader;
use super::mask::{Mask, Masker};

/// A text string or a block of binary data that can be sent or recevied over a WebSocket.
#[derive(Clone, Debug)]
pub struct Message {
    is_text: bool,
    data: Bytes,
}

impl Message {
    /// Creates a message from a `Bytes` object.
    ///
    /// The message can be tagged as text or binary. When the `is_text` is `true` this function validates the bytes in
    /// `data` and returns `Err` if they do not contain valid UTF-8 text.
    pub fn new(is_text: bool, data: Bytes) -> result::Result<Self, Utf8Error> {
        if is_text {
            str::from_utf8(&data)?;
        }

        Ok(Message { is_text, data })
    }

    /// Creates a text message from a `&str`.
    pub fn text(data: &str) -> Self {
        Message {
            is_text: true,
            data: data.into(),
        }
    }

    /// Creates a binary message from any type that can be converted to `Bytes`, such as `&[u8]` or `Vec<u8>`.
    pub fn binary<B: Into<Bytes>>(data: B) -> Self {
        Message {
            is_text: false,
            data: data.into(),
        }
    }

    /// Returns a reference to the data held in this message.
    pub fn data(&self) -> &Bytes {
        &self.data
    }

    /// For text messages, return a reference to the text.
    pub fn as_text(&self) -> Option<&str> {
        if self.is_text {
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
        MessageCodec { masker: Masker::new() }
    }
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>> {
        let (header, data_range) = if let Some(tuple) = FrameHeader::validate(&src)? {
            tuple
        } else {
            return Ok(None);
        };

        assert!(header.fin);

        let is_text = if header.opcode == 1 {
            true
        } else {
            assert_eq!(header.opcode, 2);
            false
        };

        let data = src.split_to(data_range.end)
            .freeze()
            .slice(data_range.start, data_range.end);

        Ok(Some(Message::new(is_text, data)?))
    }
}

impl Encoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<()> {
        let mask = Mask::new();

        let header = FrameHeader {
            fin: true,
            opcode: if item.is_text { 1 } else { 2 },
            mask: Some(mask),
            len: item.data.len(),
        };

        header.write_to(dst);
        dst.put(self.masker.mask(item.data, mask));
        Ok(())
    }
}
