use std::mem;

use bytes::{Bytes, BytesMut};
use rand;
use take_mut;

#[derive(Copy, Clone)]
pub struct Mask(u32);

impl Mask {
    pub fn new() -> Self {
        rand::random::<u32>().into()
    }
}

impl From<u32> for Mask {
    fn from(data: u32) -> Self {
        Mask(data)
    }
}

impl From<Mask> for u32 {
    fn from(mask: Mask) -> Self {
        mask.0
    }
}

pub struct Masker {
    buf: Bytes,
}

impl Masker {
    pub fn new() -> Self {
        Masker { buf: Bytes::new() }
    }

    pub fn mask(&mut self, data: Bytes, mask: Mask) -> Bytes {
        let mask = unsafe { mem::transmute::<u32, [u8; 4]>(mask.0) };
        let mask = mask.iter().cycle();
        match data.try_mut() {
            Ok(mut data) => {
                for (b, mask) in data.iter_mut().zip(mask) {
                    *b ^= mask;
                }

                data.freeze()
            }

            Err(data) => {
                take_mut::take(&mut self.buf, |buf| {
                    let mut buf = buf.try_mut().unwrap_or_else(|_old_mask_buf| BytesMut::new());

                    buf.resize(data.len(), 0);

                    for (dest, (&src, mask)) in buf.iter_mut().zip(data.iter().zip(mask)) {
                        *dest = src ^ mask;
                    }

                    buf.freeze()
                });

                self.buf.clone()
            }
        }
    }
}
