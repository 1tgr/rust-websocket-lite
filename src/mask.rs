#![allow(warnings)]
use std::mem;

use bytes::{Bytes, BytesMut};
use rand;
use take_mut;

#[derive(Copy, Clone, Debug, PartialEq)]
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

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    // Must be longer than bytes::INLINE_CAP = 31 bytes
    pub static DATA: &'static [u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

    #[test]
    fn cant_try_mut_a_shared_bytes() {
        // The benches below rely on having test data that causes `orig_data.clone().try_mut()` to return Err
        let orig_data = Bytes::from(DATA);
        let data = orig_data.clone();
        assert!(data.try_mut().is_err());
    }
}

#[cfg(all(feature = "nightly", test))]
mod benches {
    use bytes::Bytes;
    use take_mut;
    use test::Bencher;

    use super::Masker;
    use super::tests::DATA;

    #[bench]
    fn mask_not_shared(b: &mut Bencher) {
        // Given a Bytes that has never been clone()d, Masker::mask should be fast.
        let mask = 42.into();
        let mut orig_data = Bytes::from(DATA);
        b.iter(|| {
            take_mut::take(&mut orig_data, |data| {
                let mut masker = Masker::new();
                let data = masker.mask(data, mask);
                let data = masker.mask(data, mask);
                data
            });
        })
    }

    #[bench]
    fn mask_shared(b: &mut Bencher) {
        // Given a Bytes where a clone()d instance exists somewhere, Masker::mask should be
        // slower, but still reasonably fast.
        let mask = 42.into();
        let orig_data = Bytes::from(DATA);
        b.iter(|| {
            let mut masker = Masker::new();
            let data = orig_data.clone();
            let data = masker.mask(data.clone(), mask);
            let data = masker.mask(data.clone(), mask);
            assert_eq!(orig_data, data);
        });
    }
}