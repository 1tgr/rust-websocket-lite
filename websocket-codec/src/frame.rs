use std::convert::TryFrom;
use std::io;
use std::mem;
use std::result;
use std::usize;

use byteorder::{ByteOrder, NativeEndian};
use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::mask::Mask;
use crate::{Error, Result};

/// A placeholder error type for the `FrameHeaderCodec`.
///
/// Encoding and decoding of frame headers cannot return an error. This uninhabited type is assigned
/// to the `Error` associated type on `FrameHeaderCodec`.
#[derive(Copy, Clone, Debug)]
pub enum Infallible {}

impl From<io::Error> for Infallible {
    fn from(e: io::Error) -> Self {
        panic!("unexpected error: {}", e)
    }
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

#[cfg(target_endian = "little")]
fn put_u32_native(dst: &mut BytesMut, mask: u32) {
    dst.put_u32_le(mask);
}
#[cfg(target_endian = "big")]
fn put_u32_native(dst: &mut BytesMut, mask: u32) {
    dst.put_u32(mask);
}

fn get_u32_native(src: &mut &[u8]) -> u32 {
    let mask = NativeEndian::read_u32(src);
    src.advance(4);
    mask
}

/// Describes the length of the payload data within an individual WebSocket frame.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DataLength {
    /// Holds the length of a payload of 125 bytes or shorter.
    Small(u8),
    /// Holds the length of a payload between 126 and 65535 bytes.
    Medium(u16),
    /// Holds the length of a payload between 65536 and 2^63 bytes.
    Large(u64),
}

impl From<u64> for DataLength {
    fn from(n: u64) -> Self {
        if n <= 125 {
            Self::Small(n as u8)
        } else if n <= 65535 {
            Self::Medium(n as u16)
        } else {
            Self::Large(n)
        }
    }
}

impl TryFrom<DataLength> for u64 {
    type Error = Error;

    fn try_from(len: DataLength) -> Result<Self> {
        match len {
            DataLength::Small(n) => Ok(n as u64),
            DataLength::Medium(n) => {
                if n <= 125 {
                    return Err(format!("payload length {} should not be represented using 16 bits", n).into());
                }

                Ok(n as u64)
            }
            DataLength::Large(n) => {
                if n <= 65535 {
                    return Err(format!("payload length {} should not be represented using 64 bits", n).into());
                }

                if n >= 0x80000000_00000000 {
                    return Err(format!("frame is too long: {} bytes ({:x})", n, n).into());
                }

                Ok(n as u64)
            }
        }
    }
}

impl From<usize> for DataLength {
    fn from(n: usize) -> Self {
        Self::from(n as u64)
    }
}

impl TryFrom<DataLength> for usize {
    type Error = Error;

    fn try_from(len: DataLength) -> Result<Self> {
        let len = u64::try_from(len)?;
        if len > usize::MAX as u64 {
            return Err(format!(
                "frame of {} bytes can't be parsed on a {}-bit platform",
                len,
                mem::size_of::<usize>() / 8
            )
            .into());
        }

        Ok(len as usize)
    }
}

/// Describes an individual frame within a WebSocket message at a low level.
///
/// The frame header is a lower level detail of the WebSocket protocol. At the application level,
/// use [`Message`](struct.Message.html) structs and the [`MessageCodec`](struct.MessageCodec.html).
#[derive(Clone, Debug, PartialEq)]
pub struct FrameHeader {
    pub(crate) fin: bool,
    pub(crate) rsv: u8,
    pub(crate) opcode: u8,
    pub(crate) mask: Option<Mask>,
    pub(crate) data_len: DataLength,
}

impl FrameHeader {
    /// Returns a `FrameHeader` struct.
    pub fn new(fin: bool, rsv: u8, opcode: u8, mask: Option<Mask>, data_len: DataLength) -> Self {
        Self {
            fin,
            rsv,
            opcode,
            mask,
            data_len,
        }
    }

    /// Returns the WebSocket FIN bit, which indicates that this is the last frame in the message.
    pub fn fin(&self) -> bool {
        self.fin
    }

    /// Returns the WebSocket RSV1, RSV2 and RSV3 bits.
    ///
    /// The RSV bits may be used by extensions to the WebSocket protocol not exposed by this crate.
    pub fn rsv(&self) -> u8 {
        self.rsv
    }

    /// Returns the WebSocket opcode, which defines the interpretation of the frame payload data.
    pub fn opcode(&self) -> u8 {
        self.opcode
    }

    /// Returns the frame's mask.
    pub fn mask(&self) -> Option<Mask> {
        self.mask
    }

    /// Returns the length of the payload data that follows this header.
    pub fn data_len(&self) -> DataLength {
        self.data_len
    }

    /// Returns the total length of the frame header.
    ///
    /// The frame header is between 2 bytes and 10 bytes in length, depending on the presence of a mask
    /// and the length of the payload data.
    pub fn header_len(&self) -> usize {
        let mut len = 1 /* fin|opcode */ + 1 /* mask|len1 */;
        len += match self.data_len {
            DataLength::Small(_) => 0,
            DataLength::Medium(_) => 2,
            DataLength::Large(_) => 8,
        };

        if self.mask.is_some() {
            len += 4;
        }

        len
    }
}

/// Tokio codec for the low-level header portion of WebSocket frames.
/// This codec can send and receive [`FrameHeader`](struct.FrameHeader.html) structs.
///
/// The frame header is a lower level detail of the WebSocket protocol. At the application level,
/// use [`Message`](struct.Message.html) structs and the [`MessageCodec`](struct.MessageCodec.html).
pub struct FrameHeaderCodec;

impl Decoder for FrameHeaderCodec {
    type Item = FrameHeader;
    type Error = Infallible;

    fn decode(&mut self, src: &mut BytesMut) -> result::Result<Option<FrameHeader>, Infallible> {
        let buf = src.bytes();
        if buf.len() < 2 {
            return Ok(None);
        }

        let fin_opcode = buf[0];
        let mask_data_len = buf[1];
        let mut buf = &buf[2..];
        let mut header_len = 2;
        let fin = (fin_opcode & 0x80) != 0;
        let rsv = (fin_opcode & 0xf0) & !0x80;
        let opcode = fin_opcode & 0x0f;

        let data_len = match mask_data_len & 0x7f {
            127 => {
                if buf.len() < 8 {
                    return Ok(None);
                }

                header_len += 8;
                DataLength::Large(buf.get_u64())
            }
            126 => {
                if buf.len() < 2 {
                    return Ok(None);
                }

                header_len += 2;
                DataLength::Medium(buf.get_u16())
            }
            n => {
                assert!(n < 126);
                DataLength::Small(n)
            }
        };

        let mask = if mask_data_len & 0x80 == 0 {
            None
        } else {
            if buf.len() < 4 {
                return Ok(None);
            }

            header_len += 4;
            Some(get_u32_native(&mut buf).into())
        };

        let header = FrameHeader {
            fin,
            rsv,
            opcode,
            mask,
            data_len,
        };

        debug_assert_eq!(header.header_len(), header_len);
        src.advance(header_len);
        Ok(Some(header))
    }
}

impl<'a> Encoder<&'a FrameHeader> for FrameHeaderCodec {
    type Error = Infallible;

    fn encode(&mut self, item: &'a FrameHeader, dst: &mut BytesMut) -> result::Result<(), Infallible> {
        let FrameHeader {
            fin,
            rsv,
            opcode,
            mask,
            data_len,
        } = *item;

        let data_len = match data_len {
            DataLength::Small(n) => n as usize,
            DataLength::Medium(n) => n as usize,
            DataLength::Large(n) => n as usize,
        };

        dst.reserve(item.header_len() + data_len);

        let fin_bit = if fin { 0x80 } else { 0x00 };
        let mask_bit = if mask.is_some() { 0x80 } else { 0x00 };
        dst.put_u8(fin_bit | rsv | opcode);

        match DataLength::from(data_len) {
            DataLength::Small(n) => {
                dst.put_u8(mask_bit | n);
            }
            DataLength::Medium(n) => {
                dst.put_u8(mask_bit | 126);
                dst.put_u16(n);
            }
            DataLength::Large(n) => {
                dst.put_u8(mask_bit | 127);
                dst.put_u64(n);
            }
        };

        if let Some(mask) = mask {
            put_u32_native(dst, mask.into());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use assert_allocations::assert_allocated_bytes;
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    use crate::frame::{FrameHeader, FrameHeaderCodec};

    #[quickcheck]
    fn round_trips(fin: bool, is_text: bool, mask: Option<u32>, data_len: u64) {
        let header = assert_allocated_bytes(0, || FrameHeader {
            fin,
            rsv: 0,
            opcode: if is_text { 1 } else { 2 },
            mask: mask.map(|n| n.into()),
            data_len: data_len.into(),
        });

        assert_allocated_bytes(header.header_len() + data_len as usize, || {
            let mut codec = FrameHeaderCodec;
            let mut bytes = BytesMut::new();
            codec.encode(&header, &mut bytes).unwrap();
            let header_len = header.header_len();
            assert_eq!(bytes.len(), header_len);

            let header2 = codec.decode(&mut bytes).unwrap().unwrap();
            assert_eq!(header2.header_len(), header_len);
            assert_eq!(bytes.len(), 0);
            assert_eq!(header, header2)
        })
    }
}
