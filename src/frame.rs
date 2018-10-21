use std::io::{self, Cursor, Read};
use std::ops::Range;
use std::result;

use byteorder::{BigEndian, ReadBytesExt};
use bytes::{BufMut, BytesMut};

use super::Result;

pub struct FrameHeader {
    pub fin: bool,
    pub opcode: u8,
    pub mask: Option<[u8; 4]>,
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

            (fin, b & 0x0f)
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
                let mut mask = [0; 4];
                try_eof!(c.read_exact(&mut mask));
                Some(mask)
            };

            (mask, len)
        };

        let data_start = c.position() as usize;
        let data_end = data_start + len;
        if data.len() < data_end {
            return Ok(None);
        }

        let header = FrameHeader { fin, opcode, mask, len };

        Ok(Some((header, data_start..data_end)))
    }

    pub fn write_to(&self, dst: &mut BytesMut) {
        if !self.fin {
            assert_eq!(0, self.opcode);
        }

        dst.reserve(10 + self.len as usize);
        dst.put_u8((if self.fin { 0x80 } else { 0x00 }) | self.opcode);

        let mask_bit = if self.mask.is_some() { 0x80 } else { 0x00 };
        if self.len > 65535 {
            dst.put_u8(mask_bit | 127);
            dst.put_u64_be(self.len as u64);
        } else if self.len >= 126 {
            dst.put_u8(mask_bit | 126);
            dst.put_u16_be(self.len as u16);
        } else {
            dst.put_u8(mask_bit | self.len as u8);
        }

        if let Some(mask) = &self.mask {
            dst.reserve(4);
            dst.put_slice(mask);
        }
    }
}
