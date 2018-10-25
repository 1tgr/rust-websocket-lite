use std::io::{Read, Write};

use bytes::{BufMut, BytesMut};
use tokio_codec::{Decoder, Encoder};

pub struct Framed<S, C> {
    stream: S,
    codec: C,
    read_buf: BytesMut,
    write_buf: BytesMut,
}

impl<S, C> Framed<S, C> {
    pub fn new(stream: S, codec: C) -> Self {
        Framed {
            stream,
            codec,
            read_buf: BytesMut::new(),
            write_buf: BytesMut::new(),
        }
    }

    pub fn replace_codec<D>(self, codec: D) -> Framed<S, D> {
        Framed {
            stream: self.stream,
            codec,
            read_buf: self.read_buf,
            write_buf: self.write_buf,
        }
    }
}

impl<S: Write, C: Encoder> Framed<S, C> {
    pub fn send(&mut self, item: C::Item) -> Result<(), C::Error> {
        self.write_buf.truncate(0);
        self.codec.encode(item, &mut self.write_buf)?;
        self.stream.write_all(&self.write_buf)?;
        Ok(())
    }
}

impl<S: Read, C: Decoder> Framed<S, C> {
    pub fn receive(&mut self) -> Result<Option<C::Item>, C::Error> {
        loop {
            if self.read_buf.capacity() == 0 {
                self.read_buf.reserve(8 * 1024);
            } else {
                if let Some(frame) = self.codec.decode(&mut self.read_buf)? {
                    return Ok(Some(frame));
                }

                self.read_buf.reserve(1);
            }

            let n = unsafe {
                let n = self.stream.read(self.read_buf.bytes_mut())?;
                self.read_buf.advance_mut(n);
                n
            };

            if n == 0 {
                return self.codec.decode_eof(&mut self.read_buf);
            }
        }
    }
}
