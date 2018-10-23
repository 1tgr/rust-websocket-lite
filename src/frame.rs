use std::io::{self, Cursor};
use std::ops::Range;
use std::result;

use byteorder::{BigEndian, NativeEndian, ReadBytesExt};
use bytes::{BufMut, BytesMut};

use super::{Opcode, Result};
use super::mask::Mask;

#[derive(Clone, Debug, PartialEq)]
pub struct FrameHeader {
    pub fin: bool,
    pub opcode: Option<Opcode>,
    pub mask: Option<Mask>,
    pub len: usize,
}

macro_rules! try_eof {
    ($result: expr) => {{
        let result: result::Result<_, io::Error> = $result;
        match result {
            Ok(value) => value,
            Err(e) => {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return Ok(None);
                } else {
                    return Err(e.into());
                }
            }
        }
    }};
}

impl FrameHeader {
    pub fn validate(data: &[u8]) -> Result<Option<(Self, Range<usize>)>> {
        let mut c = Cursor::new(data);

        let (fin, opcode) = {
            let b = try_eof!(c.read_u8());

            let fin = match b & 0xf0 {
                0x00 => false,
                0x80 => true,
                _ => {
                    return Err("reserved bits are not supported".into());
                }
            };

            let opcode = match b & 0x0f {
                0 => None,
                n => Some(Opcode::try_from(n).ok_or_else(|| format!("opcode {} is not supported", n))?),
            };

            (fin, opcode)
        };

        let (mask, len) = {
            let b = try_eof!(c.read_u8());

            let len = match b & 0x7f {
                127 => try_eof!(c.read_u64::<BigEndian>()) as usize,
                126 => try_eof!(c.read_u16::<BigEndian>()) as usize,
                n => {
                    assert!(n < 126);
                    n as usize
                }
            };

            let mask = if b & 0x80 == 0 {
                None
            } else {
                Some(try_eof!(c.read_u32::<NativeEndian>()).into())
            };

            (mask, len)
        };

        if let Some(opcode) = opcode {
            if opcode.is_control() && len >= 126 {
                return Err(format!(
                    "control frames must be shorter than 126 bytes ({} bytes is too long)",
                    len
                ).into());
            }
        }

        let data_start = c.position() as usize;

        Ok(Some((
            FrameHeader { fin, opcode, mask, len },
            data_start..data_start + len,
        )))
    }

    fn frame_len(&self) -> usize {
        let mut len = 1 /* fin|opcode */ + 1 /* mask|len1 */;
        if self.len > 65535 {
            len += 8;
        } else if self.len > 125 {
            len += 2;
        }

        if self.mask.is_some() {
            len += 4;
        }

        len + self.len
    }

    pub fn write_to(&self, dst: &mut BytesMut) {
        let fin_bit = if self.fin { 0x80 } else { 0x00 };
        let opcode = self.opcode.map(u8::from).unwrap_or(0);
        let mask_bit = if self.mask.is_some() { 0x80 } else { 0x00 };
        dst.reserve(self.frame_len());
        dst.put_u8(fin_bit | opcode);

        if self.len > 65535 {
            dst.put_u8(mask_bit | 127);
            dst.put_u64_be(self.len as u64);
        } else if self.len > 125 {
            dst.put_u8(mask_bit | 126);
            dst.put_u16_be(self.len as u16);
        } else {
            dst.put_u8(mask_bit | self.len as u8);
        }

        if let Some(mask) = self.mask {
            #[allow(deprecated)]
            dst.put_u32::<NativeEndian>(mask.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use super::FrameHeader;
    use opcode::Opcode;

    fn round_trips(fin: bool, is_text: bool, mask: Option<u32>, len: usize) {
        let header = FrameHeader {
            fin,
            opcode: Some(if is_text { Opcode::Text } else { Opcode::Binary }),
            mask: mask.map(|n| n.into()),
            len,
        };

        let mut bytes = BytesMut::new();
        header.write_to(&mut bytes);

        let bytes = bytes.freeze();
        assert_eq!(header.frame_len(), bytes.len() + header.len);

        let (header2, data_range) = FrameHeader::validate(&bytes).unwrap().unwrap();
        assert_eq!(data_range.start, bytes.len());
        assert_eq!(header, header2)
    }

    quickcheck! {
        fn qc_round_trips(fin: bool, is_text: bool, mask: Option<u32>, len: usize) -> bool {
            round_trips(fin, is_text, mask, len);
            true
        }
    }
}
