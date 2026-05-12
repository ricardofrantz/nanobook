use std::fs::File;
use std::io::{self, BufReader, BufWriter, Error, ErrorKind, Read, Write};
use std::path::PathBuf;

const NS_PER_MINUTE: u64 = 60_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    input: PathBuf,
    output: PathBuf,
    start_ns: u64,
    duration_ns: u64,
}

fn main() -> io::Result<()> {
    let config = parse_args(std::env::args().skip(1))?;
    if config.input.as_os_str() == "-" {
        let stdin = io::stdin();
        let mut input = BufReader::new(stdin.lock());
        let mut output = BufWriter::new(File::create(&config.output)?);
        slice_itch(&mut input, &mut output, config.start_ns, config.duration_ns)
    } else {
        let mut input = BufReader::new(File::open(&config.input)?);
        let mut output = BufWriter::new(File::create(&config.output)?);
        slice_itch(&mut input, &mut output, config.start_ns, config.duration_ns)
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> io::Result<Config> {
    let mut input = None;
    let mut output = None;
    let mut start_ns = None;
    let mut duration_ns = NS_PER_MINUTE;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" | "-i" => input = Some(PathBuf::from(next_value(&mut args, &arg)?)),
            "--output" | "-o" => output = Some(PathBuf::from(next_value(&mut args, &arg)?)),
            "--start-ns" => start_ns = Some(parse_u64(&next_value(&mut args, &arg)?, &arg)?),
            "--duration-ns" => duration_ns = parse_u64(&next_value(&mut args, &arg)?, &arg)?,
            "--help" | "-h" => return Err(Error::new(ErrorKind::InvalidInput, usage())),
            other => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown argument {other}\n\n{}", usage()),
                ));
            }
        }
    }

    Ok(Config {
        input: input.ok_or_else(|| Error::new(ErrorKind::InvalidInput, usage()))?,
        output: output.ok_or_else(|| Error::new(ErrorKind::InvalidInput, usage()))?,
        start_ns: start_ns.ok_or_else(|| Error::new(ErrorKind::InvalidInput, usage()))?,
        duration_ns,
    })
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> io::Result<String> {
    args.next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, format!("{flag} requires a value")))
}

fn parse_u64(value: &str, flag: &str) -> io::Result<u64> {
    value.parse::<u64>().map_err(|err| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("{flag} must be an unsigned integer: {err}"),
        )
    })
}

fn usage() -> &'static str {
    "usage: itch-slice --input FILE --output FILE --start-ns N [--duration-ns N]"
}

fn slice_itch<R: Read, W: Write>(
    mut input: R,
    mut output: W,
    start_ns: u64,
    duration_ns: u64,
) -> io::Result<()> {
    let end_ns = start_ns
        .checked_add(duration_ns)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "slice end overflows u64"))?;

    while let Some(frame) = read_frame(&mut input)? {
        match keep_frame(&frame, start_ns, end_ns) {
            FrameDecision::Keep => output.write_all(&frame)?,
            FrameDecision::Skip => {}
            FrameDecision::Done => break,
        }
    }

    Ok(())
}

fn read_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0_u8; 2];
    match input.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }

    let len = u16::from_be_bytes(len_buf) as usize;
    if len == 0 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "ITCH message length is 0",
        ));
    }

    let mut frame = Vec::with_capacity(len + 2);
    frame.extend_from_slice(&len_buf);
    frame.resize(len + 2, 0);
    input.read_exact(&mut frame[2..])?;
    Ok(Some(frame))
}

fn keep_frame(frame: &[u8], start_ns: u64, end_ns: u64) -> FrameDecision {
    let Some(message_type) = frame.get(2).copied() else {
        return FrameDecision::Skip;
    };
    let Some(timestamp) = timestamp_ns(frame) else {
        return if is_reference_message(message_type) {
            FrameDecision::Keep
        } else {
            FrameDecision::Skip
        };
    };

    if timestamp >= end_ns {
        FrameDecision::Done
    } else if timestamp >= start_ns || is_reference_message(message_type) {
        FrameDecision::Keep
    } else {
        FrameDecision::Skip
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameDecision {
    Keep,
    Skip,
    Done,
}

fn timestamp_ns(frame: &[u8]) -> Option<u64> {
    let timestamp = frame.get(7..13)?;
    let mut bytes = [0_u8; 8];
    bytes[2..].copy_from_slice(timestamp);
    Some(u64::from_be_bytes(bytes))
}

fn is_reference_message(message_type: u8) -> bool {
    matches!(
        message_type,
        b'S' | b'R' | b'H' | b'Y' | b'L' | b'V' | b'W' | b'K' | b'J' | b'h'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: u64 = 34_200_000_000_000;

    #[test]
    fn slices_timestamped_frames_and_preserves_reference_preamble() {
        let before_add = frame(b'A', START - 1, b"before");
        let directory = frame(b'R', START - 1, b"directory");
        let first = frame(b'A', START, b"first");
        let last = frame(b'X', START + NS_PER_MINUTE - 1, b"last");
        let at_end = frame(b'D', START + NS_PER_MINUTE, b"after");
        let input = concat([
            before_add.as_slice(),
            directory.as_slice(),
            first.as_slice(),
            last.as_slice(),
            at_end.as_slice(),
        ]);

        let mut output = Vec::new();
        slice_itch(input.as_slice(), &mut output, START, NS_PER_MINUTE).unwrap();

        assert_eq!(
            output,
            concat([directory.as_slice(), first.as_slice(), last.as_slice()])
        );
    }

    #[test]
    fn reference_messages_inside_window_are_kept_once() {
        let directory = frame(b'R', START + 10, b"directory");
        let mut output = Vec::new();

        slice_itch(directory.as_slice(), &mut output, START, NS_PER_MINUTE).unwrap();

        assert_eq!(output, directory);
    }

    #[test]
    fn rejects_zero_length_frame() {
        let err = slice_itch([0, 0].as_slice(), Vec::new(), START, NS_PER_MINUTE).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_truncated_frame() {
        let err =
            slice_itch([0, 4, b'A', 1].as_slice(), Vec::new(), START, NS_PER_MINUTE).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
    }

    #[test]
    fn parses_cli_arguments() {
        let config = parse_args([
            "--input".to_string(),
            "in.itch".to_string(),
            "--output".to_string(),
            "out.itch".to_string(),
            "--start-ns".to_string(),
            "123".to_string(),
            "--duration-ns".to_string(),
            "456".to_string(),
        ])
        .unwrap();

        assert_eq!(
            config,
            Config {
                input: PathBuf::from("in.itch"),
                output: PathBuf::from("out.itch"),
                start_ns: 123,
                duration_ns: 456,
            }
        );
    }

    fn frame(message_type: u8, timestamp: u64, payload: &[u8]) -> Vec<u8> {
        let len = 1 + 2 + 2 + 6 + payload.len();
        let mut frame = Vec::with_capacity(len + 2);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
        frame.push(message_type);
        frame.extend_from_slice(&1_u16.to_be_bytes());
        frame.extend_from_slice(&2_u16.to_be_bytes());
        frame.extend_from_slice(&timestamp.to_be_bytes()[2..]);
        frame.extend_from_slice(payload);
        frame
    }

    fn concat<const N: usize>(parts: [&[u8]; N]) -> Vec<u8> {
        parts.into_iter().flatten().copied().collect()
    }
}
