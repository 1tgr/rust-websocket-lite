use std::fs::File;
use std::i64;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::result;

use bytes::{Buf, BytesMut};
use structopt::StructOpt;
use tokio_util::codec::Decoder;
use websocket_codec::protocol::{DataLength, FrameHeader, FrameHeaderCodec};
use websocket_codec::{Opcode, Result};

fn decode_stream<S: BufRead, C: Decoder>(codec: &mut C, mut stream: S) -> result::Result<Option<C::Item>, C::Error> {
    let mut prev_buf_len = 0;
    loop {
        let buf = stream.fill_buf()?;
        if buf.len() == prev_buf_len {
            return Ok(None);
        }

        prev_buf_len = buf.len();

        let mut read_buf = BytesMut::from(buf);
        let prev_remaining = read_buf.remaining();
        let result = codec.decode(&mut read_buf);
        let consumed = prev_remaining - read_buf.remaining();
        stream.consume(consumed);
        if let Some(frame) = result? {
            return Ok(Some(frame));
        }
    }
}

fn seek_forward<S: Seek>(mut stream: S, bytes: u64) -> result::Result<u64, io::Error> {
    let delta = bytes as i64;

    let delta = if delta < 0 {
        stream.seek(SeekFrom::Current(i64::MAX))?;

        let rest = (bytes - i64::MAX as u64) as i64;
        assert!(rest >= 0);
        rest
    } else {
        delta
    };

    stream.seek(SeekFrom::Current(delta))
}

fn display(header: &FrameHeader) -> String {
    let opcode = header.opcode();

    let opcode = Opcode::try_from(opcode)
        .map(|opcode| format!("{:?}", opcode))
        .unwrap_or_else(|| opcode.to_string());

    let mask = header
        .mask()
        .map(|mask| format!(", mask: 0x{:x}", u32::from(mask)))
        .unwrap_or_default();

    format!(
        "{{ fin: {}, rsv: {}, opcode: {}{}, data_len: {:?} }}",
        header.fin(),
        header.rsv(),
        opcode,
        mask,
        header.data_len()
    )
}

fn inspect(path: &Path, dump_header: bool, dump_data: bool) -> Result<()> {
    let mut stream = BufReader::new(File::open(path)?);
    let file_len = stream.seek(SeekFrom::End(0))?;
    stream.seek(SeekFrom::Start(0))?;

    while let Some(header) = decode_stream(&mut FrameHeaderCodec, &mut stream)? {
        if dump_header {
            println!("{}: {}", path.to_string_lossy(), display(&header));
        }

        let data_len = match header.data_len() {
            DataLength::Small(n) => n as u64,
            DataLength::Medium(n) => n as u64,
            DataLength::Large(n) => n as u64,
        };

        let actual_data_len = if dump_data {
            let mut stream = stream.by_ref().take(data_len);
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            io::copy(&mut stream, &mut stdout)?
        } else {
            let prev_pos = stream.seek(SeekFrom::Current(0))?;

            let pos = seek_forward(&mut stream, data_len)
                .map(|pos| pos.min(file_len))
                .unwrap_or(file_len);

            pos - prev_pos
        };

        if actual_data_len != data_len {
            return Err(format!(
                "stream contains incomplete data: expected {0} bytes (0x{0:x} bytes), got {1} bytes (0x{1:x} bytes)",
                data_len, actual_data_len
            )
            .into());
        }
    }

    let buf = stream.fill_buf()?;
    if !buf.is_empty() {
        return Err(format!("additional {} data bytes at end of stream: {:?}", buf.len(), buf).into());
    }

    Ok(())
}

#[derive(Debug, StructOpt)]
#[structopt(name = "wsinspect", about = "Inspect WebSocket protocol data")]
struct Opt {
    /// Disables display of frame headers
    #[structopt(long)]
    no_dump_header: bool,

    /// Displays frame payload data
    #[structopt(long)]
    dump_data: bool,

    #[structopt(parse(from_os_str))]
    files: Vec<PathBuf>,
}

fn main() {
    let Opt {
        files,
        no_dump_header,
        dump_data,
    } = Opt::from_args();

    for path in files {
        if let Err(e) = inspect(&path, !no_dump_header, dump_data) {
            eprintln!("{}: {}", path.to_string_lossy(), e);
        }
    }
}
