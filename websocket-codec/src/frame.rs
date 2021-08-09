use std::convert::TryFrom;
use std::{mem, usize};

use byteorder::{BigEndian, ByteOrder, NativeEndian};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::mask::Mask;
use crate::{Error, Result};

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
    #[allow(clippy::cast_possible_truncation)]
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

    #[allow(clippy::cast_possible_truncation)]
    fn try_from(len: DataLength) -> Result<Self> {
        match len {
            DataLength::Small(n) => Ok(u64::from(n)),
            DataLength::Medium(n) => {
                if n <= 125 {
                    return Err(format!("payload length {} should not be represented using 16 bits", n).into());
                }

                Ok(u64::from(n))
            }
            DataLength::Large(n) => {
                if n <= 65535 {
                    return Err(format!("payload length {} should not be represented using 64 bits", n).into());
                }

                if n >= 0x8000_0000_0000_0000 {
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

    #[allow(clippy::cast_possible_truncation)]
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
    #[must_use]
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
    #[must_use]
    pub fn fin(&self) -> bool {
        self.fin
    }

    /// Returns the WebSocket RSV1, RSV2 and RSV3 bits.
    ///
    /// The RSV bits may be used by extensions to the WebSocket protocol not exposed by this crate.
    #[must_use]
    pub fn rsv(&self) -> u8 {
        self.rsv
    }

    /// Returns the WebSocket opcode, which defines the interpretation of the frame payload data.
    #[must_use]
    pub fn opcode(&self) -> u8 {
        self.opcode
    }

    /// Returns the frame's mask.
    #[must_use]
    pub fn mask(&self) -> Option<Mask> {
        self.mask
    }

    /// Returns the length of the payload data that follows this header.
    #[must_use]
    pub fn data_len(&self) -> DataLength {
        self.data_len
    }

    /// Returns the total length of the frame header.
    ///
    /// The frame header is between 2 bytes and 10 bytes in length, depending on the presence of a mask
    /// and the length of the payload data.
    #[must_use]
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

    pub(crate) fn parse_slice(buf: &[u8]) -> Option<(Self, usize)> {
        if buf.len() < 2 {
            return None;
        }

        let fin_opcode = buf[0];
        let mask_data_len = buf[1];
        let mut header_len = 2;
        let fin = (fin_opcode & 0x80) != 0;
        let rsv = (fin_opcode & 0xf0) & !0x80;
        let opcode = fin_opcode & 0x0f;

        let (buf, data_len) = match mask_data_len & 0x7f {
            127 => {
                if buf.len() < 10 {
                    return None;
                }

                header_len += 8;

                (&buf[10..], DataLength::Large(BigEndian::read_u64(&buf[2..10])))
            }
            126 => {
                if buf.len() < 4 {
                    return None;
                }

                header_len += 2;

                (&buf[4..], DataLength::Medium(BigEndian::read_u16(&buf[2..4])))
            }
            n => {
                assert!(n < 126);
                (&buf[2..], DataLength::Small(n))
            }
        };

        let mask = if mask_data_len & 0x80 == 0 {
            None
        } else {
            if buf.len() < 4 {
                return None;
            }

            header_len += 4;
            Some(NativeEndian::read_u32(buf).into())
        };

        let header = Self {
            fin,
            rsv,
            opcode,
            mask,
            data_len,
        };

        debug_assert_eq!(header.header_len(), header_len);
        Some((header, header_len))
    }

    pub(crate) fn write_to_slice(&self, dst: &mut [u8]) {
        let FrameHeader {
            fin,
            rsv,
            opcode,
            mask,
            data_len,
        } = *self;

        let mut fin_opcode = rsv | opcode;
        if fin {
            fin_opcode |= 0x80;
        };

        dst[0] = fin_opcode;

        let mask_bit = if mask.is_some() { 0x80 } else { 0 };

        let dst = match data_len {
            DataLength::Small(n) => {
                dst[1] = mask_bit | n;
                &mut dst[2..]
            }
            DataLength::Medium(n) => {
                let (dst, rest) = dst.split_at_mut(4);
                dst[1] = mask_bit | 126;
                BigEndian::write_u16(&mut dst[2..4], n);
                rest
            }
            DataLength::Large(n) => {
                let (dst, rest) = dst.split_at_mut(10);
                dst[1] = mask_bit | 127;
                BigEndian::write_u64(&mut dst[2..10], n);
                rest
            }
        };

        if let Some(mask) = mask {
            NativeEndian::write_u32(dst, mask.into());
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn write_to_bytes(&self, dst: &mut BytesMut) {
        let data_len = match self.data_len {
            DataLength::Small(n) => n as usize,
            DataLength::Medium(n) => n as usize,
            DataLength::Large(n) => n as usize,
        };

        let initial_len = dst.len();
        let header_len = self.header_len();
        dst.reserve(header_len + data_len);

        unsafe {
            dst.set_len(initial_len + header_len);
        }

        let dst_slice = &mut dst[initial_len..(initial_len + header_len)];
        self.write_to_slice(dst_slice);
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
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<FrameHeader>> {
        use bytes::Buf;

        Ok(FrameHeader::parse_slice(src.chunk()).map(|(header, header_len)| {
            src.advance(header_len);
            header
        }))
    }
}

impl Encoder<FrameHeader> for FrameHeaderCodec {
    type Error = Error;

    fn encode(&mut self, item: FrameHeader, dst: &mut BytesMut) -> Result<()> {
        self.encode(&item, dst)
    }
}

impl<'a> Encoder<&'a FrameHeader> for FrameHeaderCodec {
    type Error = Error;

    fn encode(&mut self, item: &'a FrameHeader, dst: &mut BytesMut) -> Result<()> {
        item.write_to_bytes(dst);
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
    fn round_trips(fin: bool, is_text: bool, mask: Option<u32>, data_len: u16) {
        let header = assert_allocated_bytes(0, || FrameHeader {
            fin,
            rsv: 0,
            opcode: if is_text { 1 } else { 2 },
            mask: mask.map(Into::into),
            data_len: u64::from(data_len).into(),
        });

        assert_allocated_bytes((header.header_len() + data_len as usize).max(8), || {
            let mut codec = FrameHeaderCodec;
            let mut bytes = BytesMut::new();
            codec.encode(&header, &mut bytes).unwrap();
            let header_len = header.header_len();
            assert_eq!(bytes.len(), header_len);

            let header2 = codec.decode(&mut bytes).unwrap().unwrap();
            assert_eq!(header2.header_len(), header_len);
            assert_eq!(bytes.len(), 0);
            assert_eq!(header, header2);
        });
    }
}
