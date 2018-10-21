use bytes::{Bytes, BytesMut};
use take_mut;

pub struct Masker {
    buf: Bytes,
}

impl Masker {
    pub fn new() -> Self {
        Masker { buf: Bytes::new() }
    }

    pub fn mask<'a, M: IntoIterator<Item = &'a u8>>(&mut self, data: Bytes, mask: M) -> Bytes {
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
