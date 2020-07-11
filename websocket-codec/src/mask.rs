#![allow(clippy::new_without_default)]

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

/// Masks *by copying* data sent by a client, and unmasks data received by a server.
pub fn mask_slice_copy(buf: &mut [u8], data: &[u8], Mask(mask): Mask) {
    assert_eq!(buf.len(), data.len());

    let (buf1, buf2, buf3) = unsafe { buf.align_to_mut() };
    let (data1, data) = data.split_at(buf1.len());
    let (data_pre, data2, data3) = unsafe { data.align_to() };
    if data_pre.is_empty() {
        let mask = mask_u8_copy(buf1, data1, mask);
        mask_aligned_copy(buf2, data2, mask);
        mask_u8_copy(buf3, data3, mask);
    } else {
        let (data2, data3) = data.split_at(buf2.len() * 4);
        let mask = mask_u8_copy(buf1, data1, mask);
        mask_unaligned_copy(buf2, data2, mask);
        mask_u8_copy(buf3, data3, mask);
    }
}

fn mask_aligned_copy(buf: &mut [u32], data: &[u32], mask: u32) {
    assert_eq!(buf.len(), data.len());

    for (dest, src) in buf.iter_mut().zip(data) {
        *dest = src ^ mask;
    }
}

fn mask_unaligned_copy(buf: &mut [u32], data: &[u8], mask: u32) {
    let data = data.chunks_exact(4);
    assert_eq!(data.len(), buf.len());
    assert_eq!(data.remainder().len(), 0);

    for (dest, src) in buf.iter_mut().zip(data) {
        #[allow(clippy::cast_ptr_alignment)]
        let src = unsafe { (src.as_ptr() as *const u32).read_unaligned() };
        *dest = src ^ mask;
    }
}

fn mask_u8_copy(buf: &mut [u8], data: &[u8], mut mask: u32) -> u32 {
    assert!(data.len() < 4);
    assert_eq!(buf.len(), data.len());

    for (dest, &src) in buf.iter_mut().zip(data) {
        *dest = src ^ (mask as u8);
        mask = mask.rotate_right(8);
    }

    mask
}

/// Masks data sent by a client, and unmasks data received by a server.
pub fn mask_slice(data: &mut [u8], Mask(mask): Mask) {
    let (data1, data2, data3) = unsafe { data.align_to_mut() };
    let mask = mask_u8_in_place(data1, mask);
    mask_aligned_in_place(data2, mask);
    mask_u8_in_place(data3, mask);
}

fn mask_u8_in_place(data: &mut [u8], mut mask: u32) -> u32 {
    assert!(data.len() < 4);

    for b in data {
        *b ^= mask as u8;
        mask = mask.rotate_right(8);
    }

    mask
}

fn mask_aligned_in_place(data: &mut [u32], mask: u32) {
    for n in data {
        *n ^= mask;
    }
}

#[cfg(test)]
mod tests {
    use assert_allocations::assert_allocated_bytes;
    use bytes::{BufMut, Bytes, BytesMut};

    use crate::mask::{self, Mask};

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
        let mask = Mask::from(0xff000001u32.to_be());
        let orig_data = Bytes::from_static(DATA);

        let mut data = BytesMut::with_capacity(orig_data.len());
        data.put(orig_data.clone());
        assert_allocated_bytes(0, || mask::mask_slice(&mut data, mask));

        assert_eq!(b'a' ^ 0xff, data[0]);
        assert_eq!(b'd' ^ 0x01, data[3]);
        assert_eq!(MASKED_DATA, &data);

        assert_allocated_bytes(0, || mask::mask_slice(&mut data, mask));
        assert_eq!(orig_data, data);
    }
}
