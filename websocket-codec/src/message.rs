use std::convert::TryFrom;
use std::{str, usize};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::close::{CloseCode, CloseFrame};
use crate::frame::FrameHeader;
use crate::mask::Mask;
use crate::opcode::Opcode;
use crate::{mask, Error, Result};

/// A text string, a block of binary data or a WebSocket control frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    opcode: Opcode,
    data: Bytes,
}

impl Message {
    /// Creates a message from a [`Bytes`] object.
    ///
    /// The message can be tagged as text or binary.
    ///
    /// # Errors
    ///
    /// This function validates the bytes in `data` according to the `opcode` parameter:
    /// - For [`Opcode::Text`] it returns `Err` if the bytes in `data` do not contain valid UTF-8 text.
    /// - For [`Opcode::Close`] it returns `Err` if `data` does not contain a two-byte close code
    ///   followed by valid UTF-8 text, unless `data` is empty.
    pub fn new<B: Into<Bytes>>(opcode: Opcode, data: B) -> Result<Self> {
        let data = data.into();

        match opcode {
            Opcode::Close => match data.len() {
                0 => {}
                1 => {
                    return Err("close frames must be at least 2 bytes long".into());
                }
                _ => {
                    str::from_utf8(&data[2..])?;
                }
            },
            Opcode::Text => {
                str::from_utf8(&data)?;
            }
            _ => {}
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

    /// Creates a binary message from any type that can be converted to [`Bytes`], such as `&[u8]` or `Vec<u8>`.
    pub fn binary<B: Into<Bytes>>(data: B) -> Self {
        Message {
            opcode: Opcode::Binary,
            data: data.into(),
        }
    }

    pub(crate) fn header(&self, mask: Option<Mask>) -> FrameHeader {
        FrameHeader {
            fin: true,
            rsv: 0,
            opcode: self.opcode.into(),
            mask,
            data_len: self.data.len().into(),
        }
    }

    /// Creates a message that indicates the connection is about to be closed.
    /// The close frame does not contain a reason.
    #[must_use]
    pub fn close() -> Self {
        Message {
            opcode: Opcode::Close,
            data: Bytes::new(),
        }
    }

    /// Creates a message that indicates the connection is about to be closed.
    /// The close frame contains a code and a text reason.
    #[must_use]
    pub fn close_with_reason(code: CloseCode, mut reason: String) -> Self {
        // Shorten the string so that 2 + reason.len() fits under the limit for control frames
        let reason_len = truncate_floor_char_boundary(&mut reason, 123);

        let mut data = reason.into_bytes();
        data.extend_from_slice(&[0, 0]);
        data.copy_within(0..reason_len, 2);
        data[0..2].copy_from_slice(&u16::from(code).to_be_bytes());

        Message {
            opcode: Opcode::Close,
            data: data.into(),
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

    /// For messages with opcode [`Opcode::Text`], returns a reference to the text.
    /// Returns `None` otherwise.
    pub fn as_text(&self) -> Option<&str> {
        if self.opcode.is_text() {
            Some(unsafe { str::from_utf8_unchecked(&self.data) })
        } else {
            None
        }
    }

    /// For messages with opcode [`Opcode::Close`], returns the [`CloseFrame`].
    /// Returns `None` otherwise.
    pub fn as_close(&self) -> Option<CloseFrame> {
        if matches!(self.opcode, Opcode::Close) && self.data.len() >= 2 {
            let mut data = self.data.clone();
            let code = data.get_u16();
            Some(CloseFrame {
                code: code.into(),
                reason: data,
            })
        } else {
            None
        }
    }
}

/// Tokio codec for WebSocket messages. This codec can send and receive [`Message`] structs.
#[derive(Clone)]
pub struct MessageCodec {
    interrupted_message: Option<(Opcode, BytesMut)>,
    use_mask: bool,
}

impl MessageCodec {
    /// Creates a `MessageCodec` for a client.
    ///
    /// Encoded messages are masked.
    #[must_use]
    pub fn client() -> Self {
        Self::with_masked_encode(true)
    }

    /// Creates a `MessageCodec` for a server.
    ///
    /// Encoded messages are not masked.
    #[must_use]
    pub fn server() -> Self {
        Self::with_masked_encode(false)
    }

    /// Creates a `MessageCodec` while specifying whether to use message masking while encoding.
    #[must_use]
    pub fn with_masked_encode(use_mask: bool) -> Self {
        Self {
            use_mask,
            interrupted_message: None,
        }
    }
}

fn truncate_floor_char_boundary(s: &mut String, new_len: usize) -> usize {
    // TODO call str::floor_char_boundary when stable
    let mut len = s.len();
    if len > new_len {
        len = new_len;

        while !s.is_char_boundary(len) {
            len -= 1;
        }

        s.truncate(len);
    }

    len
}

impl Decoder for MessageCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Message>> {
        let mut state = self.interrupted_message.take();
        let (opcode, data) = loop {
            let (header, header_len) = if let Some(tuple) = FrameHeader::parse_slice(src) {
                tuple
            } else {
                // The buffer isn't big enough for the frame header. Reserve additional space for a frame header,
                // plus reasonable extensions.
                src.reserve(512);
                self.interrupted_message = state;
                return Ok(None);
            };

            let data_len = usize::try_from(header.data_len)?;
            let frame_len = header_len + data_len;
            if frame_len > src.remaining() {
                // The buffer contains the frame header but it's not big enough for the data. Reserve additional
                // space for the frame data, plus the next frame header.
                // Note that we guard against bad data that indicates an unreasonable frame length.

                // If we reserved buffer space for the entire frame data in a single call, would the buffer exceed
                // usize::MAX bytes in size?
                // On a 64-bit platform we should not reach here as the usize::try_from line above enforces the
                // max payload length detailed in the RFC of 2^63 bytes.
                if frame_len > usize::MAX - src.remaining() {
                    return Err(format!("frame is too long: {0} bytes ({0:x})", frame_len).into());
                }

                // We don't really reserve space for the entire frame data in a single call. If somebody is sending
                // more than a gigabyte of data in a single frame then we'll still try to receive it, we'll just
                // reserve in 1GB chunks.
                src.reserve(frame_len.min(0x4000_0000) + 512);

                self.interrupted_message = state;
                return Ok(None);
            }

            // The buffer contains the frame header and all of the data. We can parse it and return Ok(Some(...)).
            let mut data = src.split_to(frame_len);
            data.advance(header_len);

            let FrameHeader {
                fin,
                rsv,
                opcode,
                mask,
                data_len: _data_len,
            } = header;

            if rsv != 0 {
                return Err(format!("reserved bits are not supported: 0x{:x}", rsv).into());
            }

            if let Some(mask) = mask {
                // Note: clients never need decode masked messages because masking is only used for client -> server frames.
                // However this code is used to test round tripping of masked messages.
                mask::mask_slice(&mut data, mask);
            };

            let opcode = if opcode == 0 {
                None
            } else {
                let opcode = Opcode::try_from(opcode).ok_or_else(|| format!("opcode {} is not supported", opcode))?;
                if opcode.is_control() && data_len >= 126 {
                    return Err(format!(
                        "control frames must be shorter than 126 bytes ({} bytes is too long)",
                        data_len
                    )
                    .into());
                }

                Some(opcode)
            };

            state = if let Some((partial_opcode, mut partial_data)) = state {
                if let Some(opcode) = opcode {
                    if fin && opcode.is_control() {
                        self.interrupted_message = Some((partial_opcode, partial_data));
                        break (opcode, data);
                    }

                    return Err(format!("continuation frame must have continuation opcode, not {:?}", opcode).into());
                }

                partial_data.extend_from_slice(&data);

                if fin {
                    break (partial_opcode, partial_data);
                }

                Some((partial_opcode, partial_data))
            } else if let Some(opcode) = opcode {
                if fin {
                    break (opcode, data);
                }
                if opcode.is_control() {
                    return Err("control frames must not be fragmented".into());
                }
                Some((opcode, data))
            } else {
                return Err("continuation must not be first frame".into());
            }
        };

        Ok(Some(Message::new(opcode, data.freeze())?))
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<()> {
        self.encode(&item, dst)
    }
}

impl<'a> Encoder<&'a Message> for MessageCodec {
    type Error = Error;

    fn encode(&mut self, item: &Message, dst: &mut BytesMut) -> Result<()> {
        let mask = if self.use_mask { Some(Mask::new()) } else { None };
        let header = item.header(mask);
        header.write_to_bytes(dst);

        if let Some(mask) = mask {
            let offset = dst.len();
            dst.reserve(item.data.len());

            unsafe {
                dst.set_len(offset + item.data.len());
            }

            mask::mask_slice_copy(&mut dst[offset..], &item.data, mask);
        } else {
            dst.put_slice(&item.data);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use assert_allocations::assert_allocated_bytes;
    use bytes::{BufMut, BytesMut};
    use quickcheck::{Arbitrary, Gen};
    use tokio_util::codec::{Decoder, Encoder};

    use crate::frame::{FrameHeader, FrameHeaderCodec};
    use crate::mask;
    use crate::mask::Mask;
    use crate::message::{Message, MessageCodec};

    #[derive(Clone, Debug)]
    enum MessageInput {
        Binary { data: Vec<u8> },
        Close,
        CloseWithReason { code: u16, reason: String },
        Text { data: String },
    }

    impl Arbitrary for MessageInput {
        fn arbitrary(g: &mut Gen) -> Self {
            match u8::arbitrary(g) % 4 {
                0 => Self::Binary {
                    data: Arbitrary::arbitrary(g),
                },
                1 => Self::Close,
                2 => {
                    let (code, mut reason) = <(u16, String)>::arbitrary(g);
                    super::truncate_floor_char_boundary(&mut reason, 123); // so that 2 + reason.len() fits under the limit for control frames
                    Self::CloseWithReason { code, reason }
                }
                _ => Self::Text {
                    data: Arbitrary::arbitrary(g),
                },
            }
        }

        fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
            match self.clone() {
                Self::Binary { data } => Box::new(data.shrink().map(|data| Self::Binary { data })),
                Self::Close => quickcheck::empty_shrinker(),
                Self::CloseWithReason { code, reason } => Box::new(
                    (code, reason)
                        .shrink()
                        .map(|(code, reason)| Self::CloseWithReason { code, reason }),
                ),
                Self::Text { data } => Box::new(data.shrink().map(|data| Self::Text { data })),
            }
        }
    }

    impl MessageInput {
        fn data_len(&self) -> usize {
            match self {
                Self::Binary { data } => data.len(),
                Self::Close => 0,
                Self::CloseWithReason { code: _, reason } => 2 + reason.len(),
                Self::Text { data } => data.len(),
            }
        }

        fn into_message(self) -> Message {
            match self {
                Self::Text { data } => Message::text(data),
                Self::Close => Message::close(),
                Self::CloseWithReason { code, reason } => Message::close_with_reason(code.into(), reason),
                Self::Binary { data } => Message::binary(data),
            }
        }
    }

    #[quickcheck]
    fn round_trips(mut input: MessageInput) {
        let data_len = input.data_len();
        if let MessageInput::CloseWithReason { reason, .. } = &mut input {
            reason.reserve_exact(2); // to ensure no allocations when reason is passed to Message::close_with_reason
        }

        let message = assert_allocated_bytes(0, || input.into_message());

        // thread_rng performs a one-off memory allocation the first time it is used on a given thread.
        // We make that allocation here, instead of inside the assert_allocated_bytes block below.
        rand::thread_rng();

        let header = message.header(Some(Mask::from(0)));
        let frame_len = header.header_len() + data_len;
        let mut bytes = BytesMut::new();
        assert_allocated_bytes(frame_len.max(8), {
            || {
                MessageCodec::client()
                    .encode(&message, &mut bytes)
                    .expect("didn't expect MessageCodec::encode to return an error");
            }
        });

        // We eagerly promote the BytesMut to KIND_ARC. This ensures we make a call to Box::new here,
        // instead of inside the assert_allocated_bytes(0) block below.
        let mut src = bytes.split();

        let message2 = assert_allocated_bytes(0, || {
            MessageCodec::client()
                .decode(&mut src)
                .expect("didn't expect MessageCodec::decode to return an error")
                .expect("expected buffer to contain the full frame")
        });

        assert_eq!(message, message2);
    }

    #[quickcheck]
    #[allow(clippy::needless_pass_by_value)] // clippy wants &str, but quickcheck can only give us String
    fn round_trips_via_frame_header(is_text: bool, mask: Option<u32>, data: String) {
        let header = assert_allocated_bytes(0, || {
            FrameHeader {
                fin: true, // TODO test messages split across frames
                rsv: 0,
                opcode: if is_text { 1 } else { 2 },
                mask: mask.map(Into::into),
                data_len: data.len().into(),
            }
        });

        let mut bytes = BytesMut::with_capacity(header.header_len() + data.len());
        assert_allocated_bytes(0, || {
            FrameHeaderCodec.encode(&header, &mut bytes).unwrap();

            if let Some(mask) = header.mask {
                let offset = bytes.len();
                bytes.resize(offset + data.len(), 0);
                mask::mask_slice_copy(&mut bytes[offset..], data.as_bytes(), mask);
            } else {
                bytes.put(data.as_bytes());
            }
        });

        // We eagerly promote the BytesMut to KIND_ARC. This ensures we make a call to Box::new here,
        // instead of inside the assert_allocated_bytes(0) block below.
        let mut src = bytes.split();

        assert_allocated_bytes(0, || {
            let message2 = MessageCodec::client()
                .decode(&mut src)
                .expect("didn't expect MessageCodec::decode to return an error")
                .expect("expected buffer to contain the full frame");

            assert_eq!(is_text, message2.as_text().is_some());
            assert_eq!(data.as_bytes(), message2.data());
        });
    }

    #[quickcheck]
    fn reserves_buffer(is_text: bool, data: String) {
        let message = if is_text {
            Message::text(data)
        } else {
            Message::binary(data.into_bytes())
        };

        let mut bytes = BytesMut::new();
        MessageCodec::client()
            .encode(&message, &mut bytes)
            .expect("didn't expect MessageCodec::encode to return an error");

        // We don't check allocations around the MessageCodec::decode call below. We're deliberately
        // supplying a minimal number of source bytes each time, so we expect lots of small
        // allocations as decoder_buf is resized multiple times.

        let mut src = &bytes[..];
        let mut decoder = MessageCodec::client();
        let mut decoder_buf = BytesMut::new();
        let message2 = loop {
            if let Some(result) = decoder
                .decode(&mut decoder_buf)
                .expect("didn't expect MessageCodec::decode to return an error")
            {
                assert_eq!(0, decoder_buf.len(), "expected decoder to consume the whole buffer");
                break result;
            }

            let n = decoder_buf.remaining_mut().min(src.len());
            assert!(n > 0, "expected decoder to reserve at least one byte");
            decoder_buf.put_slice(&src[..n]);
            src = &src[n..];
        };

        assert_eq!(message, message2);
    }

    #[test]
    fn frame_bigger_than_2_64_does_not_panic() {
        // A frame with data longer than 2^64 bytes is bigger than the entire address space,
        // when the header is included.
        let data: &[u8] = &[0, 127, 255, 255, 255, 255, 255, 255, 255, 255];
        let mut data = BytesMut::from(data);
        data.resize(4096, 0);

        let message = MessageCodec::client()
            .decode(&mut data)
            .expect_err("expected decoder to return an error given a frame bigger than 2^64 bytes");

        assert_eq!(
            message.to_string(),
            "frame is too long: 18446744073709551615 bytes (ffffffffffffffff)"
        );
    }

    #[test]
    fn frame_bigger_than_2_40_does_not_panic() {
        // A frame longer than 2^40 bytes causes Vec::extend to trigger an error in
        // the AddressSanitizer.
        let data: &[u8] = &[0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 0];
        let mut data = BytesMut::from(data);
        data.resize(4096, 0);

        let message = MessageCodec::client()
            .decode(&mut data)
            .expect_err("expected decoder to return an error given a frame bigger than 2^40 bytes");

        assert_eq!(
            message.to_string(),
            "frame is too long: 18446744069414584575 bytes (ffffffff000000ff)"
        );
    }

    #[test]
    fn roundtrips_multiple_messages() {
        // According to https://docs.rs/tokio-util/0.7.3/tokio_util/codec/index.html#the-encoder-trait
        // the buffer given to the Encoder may already contain data.
        // Therefore, we check whether writing two messages into the same buffer roundtrips correctly.
        let mut buf = BytesMut::new();
        let mut codec = MessageCodec::server();
        codec.encode(Message::text("A"), &mut buf).unwrap();
        codec.encode(Message::text("B"), &mut buf).unwrap();
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap(), Message::text("A"));
        assert_eq!(codec.decode(&mut buf).unwrap().unwrap(), Message::text("B"));
    }
}
