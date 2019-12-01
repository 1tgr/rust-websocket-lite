#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default_derive))]
// use std::mem;
// use std::slice;

use bytes::{BufMut, Bytes, BytesMut};
use rand;

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

/*
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
}*/

fn mask_u8_in_place(data: &mut [u8], mut mask: u32) -> u32 {
    for b in data {
        *b ^= mask as u8;
        mask = mask.rotate_right(8);
    }

    mask
}

/*
fn mask_u8_copy(buf: &mut [u8], data: &[u8], mut mask: u32) -> u32 {
    assert_eq!(buf.len(), data.len());

    for (dest, &src) in buf.into_iter().zip(data) {
        *dest = src ^ (mask as u8);
        mask = mask.rotate_right(8);
    }

    mask
}*/

fn mask_aligned_in_place(data: &mut [u32], mask: u32) {
    for n in data {
        *n ^= mask;
    }
}
/*
fn mask_aligned_copy(buf: &mut [u32], data: &[u32], mask: u32) {
    assert_eq!(buf.len(), data.len());

    for (dest, &src) in buf.into_iter().zip(data) {
        *dest = src ^ mask;
    }
}*/

/// Masks data sent by a client, and unmasks data received by a server.
pub struct Masker;

impl Masker {
    pub fn new() -> Self {
        Self
    }

    pub fn mask(&mut self, data: Bytes, mask: Mask) -> Bytes {
        let Mask(mask) = mask;
        let mut buff = BytesMut::with_capacity(data.len());
        buff.put(data);

        let (data1, data2, data3) = unsafe { buff.align_to_mut() };
        let mask = mask_u8_in_place(data1, mask);
        mask_aligned_in_place(data2, mask);
        mask_u8_in_place(data3, mask);

        buff.freeze()
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use bytes::Bytes;

    use crate::mask::{Mask, Masker};

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
    fn can_mask() {
        let mask = Mask::from(unsafe { mem::transmute::<[u8; 4], u32>([0xff, 0x00, 0x00, 0x01]) });
        let mut masker = Masker::new();
        let orig_data = Bytes::from_static(DATA);
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

    use crate::tests::DATA;
    use crate::Masker;

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
