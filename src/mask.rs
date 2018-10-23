#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default_derive))]
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

pub fn mask_u8_in_place(data: &mut [u8], mut mask: u32) -> u32 {
    for b in data {
        *b ^= mask as u8;
        mask = mask.rotate_right(8);
    }

    mask
}

pub fn mask_u8_copy(buf: &mut [u8], data: &[u8], mut mask: u32) -> u32 {
    debug_assert_eq!(buf.len(), data.len());

    for (dest, &src) in buf.into_iter().zip(data) {
        *dest = src ^ (mask as u8);
        mask = mask.rotate_right(8);
    }

    mask
}

#[cfg(feature = "nightly")]
mod inner {
    use std::mem;
    use std::slice;

    use super::{mask_u8_copy, mask_u8_in_place};

    unsafe fn unaligned<T>(data: &[u8]) -> (&[T], &[u8]) {
        let size = mem::size_of::<T>();
        if size == 0 {
            return (&[], data);
        }

        let len1 = data.len() / size;
        (
            slice::from_raw_parts(data.as_ptr() as *const T, len1),
            &data[len1 * size..],
        )
    }

    fn mask_aligned_in_place(data: &mut [u32], mask: u32) {
        for n in data {
            *n ^= mask;
        }
    }

    fn mask_aligned_copy(buf: &mut [u32], data: &[u32], mask: u32) {
        debug_assert_eq!(buf.len(), data.len());

        for (dest, &src) in buf.into_iter().zip(data) {
            *dest = src ^ mask;
        }
    }

    pub fn mask_in_place(data: &mut [u8], mask: u32) {
        let (data1, data2, data3) = unsafe { data.align_to_mut() };
        let mask = mask_u8_in_place(data1, mask);
        mask_aligned_in_place(data2, mask);
        mask_u8_in_place(data3, mask);
    }

    pub fn mask_copy(buf: &mut [u8], data: &[u8], mask: u32) {
        let (buf1, buf2, buf3) = unsafe { buf.align_to_mut() };
        let (data1, data) = data.split_at(buf1.len());
        let (data2, data3) = unsafe { unaligned(data) };
        let mask = mask_u8_copy(buf1, data1, mask);
        mask_aligned_copy(buf2, data2, mask);
        mask_u8_copy(buf3, data3, mask);
    }
}

#[cfg(not(feature = "nightly"))]
mod inner {
    use super::{mask_u8_copy, mask_u8_in_place};

    pub fn mask_in_place(data: &mut [u8], mask: u32) {
        mask_u8_in_place(data, mask);
    }

    pub fn mask_copy(buf: &mut [u8], data: &[u8], mask: u32) {
        mask_u8_copy(buf, data, mask);
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
        match data.try_mut() {
            Ok(mut data) => {
                inner::mask_in_place(&mut data, mask.0);
                data.freeze()
            }

            Err(data) => {
                take_mut::take(&mut self.buf, |buf| {
                    let mut buf = buf.try_mut().unwrap_or_else(|_old_mask_buf| BytesMut::new());
                    buf.resize(data.len(), 0);
                    inner::mask_copy(&mut buf, &data, mask.0);
                    buf.freeze()
                });

                self.buf.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use bytes::Bytes;

    use super::{Mask, Masker};

    // Test data chosen so that:
    //  - It's not a multiple of 4, ie masking of the unaligned section works
    //  - It's longer than bytes::INLINE_CAP = 31 bytes, to force Bytes to make a memory allocation
    //
    // Mask chosen so that, per block of four bytes:
    //  - First byte has all its bits flipped, so it appears in text as an \x sequence higher than \x80
    //  - Second and third bytes are unchanged
    //  - Fourth byte has its bottom bit flipped, so in text it's still a recognisable letter

    pub static DATA: &'static [u8] = b"abcdefghijklmnopqrstuvwxyz123456789";

    static MASKED_DATA: &'static [u8] = b"\
        \x9ebce\
        \x9afgi\
        \x96jkm\
        \x92noq\
        \x8ersu\
        \x8avwy\
        \x86z13\
        \xcc457\
        \xc889";

    #[test]
    fn cant_try_mut_a_shared_bytes() {
        // The benches below rely on having test data that causes `orig_data.clone().try_mut()` to return Err
        let orig_data = Bytes::from(DATA);
        let data = orig_data.clone();
        assert!(data.try_mut().is_err());
    }

    #[test]
    fn can_mask() {
        let mask = Mask::from(unsafe { mem::transmute::<[u8; 4], u32>([0xff, 0x00, 0x00, 0x01]) });
        let mut masker = Masker::new();
        let orig_data = Bytes::from(DATA);
        let data = masker.mask(orig_data.clone(), mask);

        assert_eq!(b'a' ^ 0xff, data[0]);
        assert_eq!(b'd' ^ 0x01, data[3]);
        assert_eq!(MASKED_DATA, &data);

        let data = masker.mask(data, mask);
        assert_eq!(orig_data, data);
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
