//! `DeepSeek` Harness concatenated-frame Zstandard decoding.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use zstd::stream::raw::{Decoder as RawDecoder, InBuffer, Operation, OutBuffer};

const ZSTD_MAGIC: u32 = 0xFD2F_B528;
const MAX_SESSION_BYTES: usize = 128 * 1024 * 1024;
const MAX_STABLE_READ_ATTEMPTS: usize = 3;

enum FrameScan {
    Complete(usize),
    Torn,
}

enum PrefixDecodeError {
    Corrupt,
    Limit(String),
}

pub(super) fn decode_file(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = read_stable(path)?;
    let mut frames = Vec::new();
    let mut offset = 0;
    let mut torn_start = None;
    while offset < bytes.len() {
        match scan_frame(&bytes, offset)? {
            FrameScan::Complete(end) => {
                frames.push((offset, end));
                offset = end;
            }
            FrameScan::Torn => {
                torn_start = Some(offset);
                break;
            }
        }
    }
    if frames.is_empty() {
        return Err("empty or header-less Zstandard session log".to_string());
    }

    let mut plaintext = Vec::new();
    for (index, (start, end)) in frames.iter().enumerate() {
        let decoded = decode_bounded(
            &bytes[*start..*end],
            MAX_SESSION_BYTES.saturating_sub(plaintext.len()),
        )?;
        if index == 0
            && (!decoded.ends_with(b"\n")
                || decoded[..decoded.len().saturating_sub(1)].contains(&b'\n'))
        {
            return Err("first Zstandard frame is not exactly one header line".to_string());
        }
        plaintext.extend(decoded);
    }
    if !plaintext.ends_with(b"\n") {
        return Err("complete Zstandard frame contains a torn JSONL record".to_string());
    }

    if let Some(start) = torn_start {
        let remaining = MAX_SESSION_BYTES.saturating_sub(plaintext.len());
        match decode_prefix_bounded(&bytes[start..], remaining) {
            Ok(recovered) => {
                if let Some(last_newline) = recovered.iter().rposition(|byte| *byte == b'\n') {
                    plaintext.extend_from_slice(&recovered[..=last_newline]);
                }
            }
            Err(PrefixDecodeError::Corrupt) => {}
            Err(PrefixDecodeError::Limit(error)) => return Err(error),
        }
    }
    Ok(plaintext)
}

fn decode_prefix_bounded(bytes: &[u8], remaining: usize) -> Result<Vec<u8>, PrefixDecodeError> {
    let mut decoder = RawDecoder::new().map_err(|_| PrefixDecodeError::Corrupt)?;
    let mut input = InBuffer::around(bytes);
    let mut plaintext_prefix = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let before = input.pos();
        let mut output = OutBuffer::around(&mut buffer[..]);
        decoder
            .run(&mut input, &mut output)
            .map_err(|_| PrefixDecodeError::Corrupt)?;
        let written = output.pos();
        if plaintext_prefix.len().saturating_add(written) > remaining {
            return Err(PrefixDecodeError::Limit(format!(
                "decompressed DSH session exceeds the {} MiB safety limit",
                MAX_SESSION_BYTES / 1024 / 1024
            )));
        }
        plaintext_prefix.extend_from_slice(&buffer[..written]);
        if input.pos() == bytes.len() && written < buffer.len() {
            break;
        }
        if input.pos() == before && written == 0 {
            break;
        }
    }
    Ok(plaintext_prefix)
}

fn decode_bounded(bytes: &[u8], remaining: usize) -> Result<Vec<u8>, String> {
    let decoder = zstd::stream::Decoder::new(bytes)
        .map_err(|error| format!("corrupt Zstandard session frame: {error}"))?;
    let mut plaintext = Vec::new();
    decoder
        .take(
            u64::try_from(remaining)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut plaintext)
        .map_err(|error| format!("corrupt Zstandard session frame: {error}"))?;
    if plaintext.len() > remaining {
        return Err(format!(
            "decompressed DSH session exceeds the {} MiB safety limit",
            MAX_SESSION_BYTES / 1024 / 1024
        ));
    }
    Ok(plaintext)
}

#[derive(Eq, PartialEq)]
struct FileRevision {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    mtime_nsec: i64,
    #[cfg(unix)]
    ctime_nsec: i64,
}

fn revision(path: &Path) -> Result<FileRevision, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    Ok(FileRevision {
        len: metadata.len(),
        modified: metadata.modified().map_err(|error| error.to_string())?,
        #[cfg(unix)]
        dev: metadata.dev(),
        #[cfg(unix)]
        ino: metadata.ino(),
        #[cfg(unix)]
        mtime_nsec: metadata.mtime_nsec(),
        #[cfg(unix)]
        ctime_nsec: metadata.ctime_nsec(),
    })
}

fn read_snapshot(path: &Path) -> Result<(FileRevision, Vec<u8>, FileRevision), String> {
    let before = revision(path)?;
    if before.len > MAX_SESSION_BYTES as u64 {
        return Err(format!(
            "DSH session exceeds the {} MiB safety limit",
            MAX_SESSION_BYTES / 1024 / 1024
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(MAX_SESSION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_SESSION_BYTES {
        return Err(format!(
            "DSH session exceeds the {} MiB safety limit",
            MAX_SESSION_BYTES / 1024 / 1024
        ));
    }
    let after = revision(path)?;
    Ok((before, bytes, after))
}

fn read_until_stable(
    mut read_attempt: impl FnMut() -> Result<(FileRevision, Vec<u8>, FileRevision), String>,
) -> Result<Vec<u8>, String> {
    for _ in 0..MAX_STABLE_READ_ATTEMPTS {
        let (before, bytes, after) = read_attempt()?;
        if before == after {
            return Ok(bytes);
        }
    }
    Err(format!(
        "DSH session changed during {MAX_STABLE_READ_ATTEMPTS} consecutive reads"
    ))
}

pub(super) fn read_stable(path: &Path) -> Result<Vec<u8>, String> {
    read_until_stable(|| read_snapshot(path))
}

fn scan_frame(bytes: &[u8], start: usize) -> Result<FrameScan, String> {
    let mut offset = start;
    if bytes.len().saturating_sub(offset) < 4 {
        return Ok(FrameScan::Torn);
    }
    let magic = u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four-byte frame magic"),
    );
    if magic != ZSTD_MAGIC {
        return Err(format!(
            "corrupt Zstandard session log: invalid frame magic at byte {offset}"
        ));
    }
    offset += 4;
    let Some(&descriptor) = bytes.get(offset) else {
        return Ok(FrameScan::Torn);
    };
    offset += 1;
    if descriptor & 0x18 != 0 {
        return Err(format!(
            "corrupt Zstandard session log: reserved frame-header bit at byte {}",
            offset - 1
        ));
    }

    let content_size_flag = descriptor >> 6;
    let single_segment = descriptor & 0x20 != 0;
    let checksum = descriptor & 0x04 != 0;
    if !checksum {
        return Err(format!(
            "corrupt Zstandard session log: frame at byte {start} omits the required checksum"
        ));
    }
    let dictionary_bytes = match descriptor & 0x03 {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        _ => unreachable!(),
    };
    let content_size_bytes = match (content_size_flag, single_segment) {
        (0, false) => 0,
        (0, true) => 1,
        (1, _) => 2,
        (2, _) => 4,
        (3, _) => 8,
        _ => unreachable!(),
    };
    let remaining_header = usize::from(!single_segment) + dictionary_bytes + content_size_bytes;
    if bytes.len().saturating_sub(offset) < remaining_header {
        return Ok(FrameScan::Torn);
    }
    offset += remaining_header;

    loop {
        if bytes.len().saturating_sub(offset) < 3 {
            return Ok(FrameScan::Torn);
        }
        let block_header = u32::from(bytes[offset])
            | (u32::from(bytes[offset + 1]) << 8)
            | (u32::from(bytes[offset + 2]) << 16);
        offset += 3;
        let last_block = block_header & 1 != 0;
        let block_type = (block_header >> 1) & 0x03;
        if block_type == 0x03 {
            return Err(format!(
                "corrupt Zstandard session log: reserved block type at byte {}",
                offset - 3
            ));
        }
        let block_size = usize::try_from(block_header >> 3)
            .map_err(|_| "Zstandard block size overflow".to_string())?;
        let payload_size = if block_type == 0x01 { 1 } else { block_size };
        if bytes.len().saturating_sub(offset) < payload_size {
            return Ok(FrameScan::Torn);
        }
        offset += payload_size;
        if last_block {
            break;
        }
    }
    if bytes.len().saturating_sub(offset) < 4 {
        return Ok(FrameScan::Torn);
    }
    offset += 4;
    Ok(FrameScan::Complete(offset))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Write;
    use std::time::{Duration, UNIX_EPOCH};

    use super::{
        FileRevision, MAX_STABLE_READ_ATTEMPTS, PrefixDecodeError, decode_prefix_bounded,
        read_until_stable, scan_frame,
    };

    fn file_revision(len: u64) -> FileRevision {
        FileRevision {
            len,
            modified: UNIX_EPOCH + Duration::from_secs(len),
            #[cfg(unix)]
            dev: 1,
            #[cfg(unix)]
            ino: 1,
            #[cfg(unix)]
            mtime_nsec: i64::try_from(len).expect("test revision fits i64"),
            #[cfg(unix)]
            ctime_nsec: i64::try_from(len).expect("test revision fits i64"),
        }
    }

    #[test]
    fn stable_read_stops_after_the_retry_limit() {
        let calls = Cell::new(0_u64);
        let result = read_until_stable(|| {
            let before = calls.get();
            calls.set(before + 1);
            Ok((file_revision(before), Vec::new(), file_revision(before + 1)))
        });

        let error = result.expect_err("continuously changing session must stop");
        assert_eq!(calls.get(), MAX_STABLE_READ_ATTEMPTS as u64);
        assert!(error.contains("changed during"), "{error}");
    }

    #[test]
    fn frame_without_official_checksum_is_rejected() {
        let mut encoder = zstd::stream::Encoder::new(Vec::new(), 1).expect("zstd encoder");
        encoder
            .include_checksum(false)
            .expect("disable frame checksum");
        encoder.write_all(b"one record\n").expect("encode frame");
        let frame = encoder.finish().expect("finish frame");

        let Err(error) = scan_frame(&frame, 0) else {
            panic!("checksum-less frame must fail");
        };
        assert!(error.contains("required checksum"), "{error}");
    }

    #[test]
    fn torn_prefix_limit_is_not_classified_as_recoverable_corruption() {
        let mut encoder = zstd::stream::Encoder::new(Vec::new(), 1).expect("zstd encoder");
        encoder.include_checksum(true).expect("enable checksum");
        encoder
            .write_all(b"complete line before torn checksum\n")
            .expect("encode prefix");
        let mut frame = encoder.finish().expect("finish frame");
        frame.pop();
        assert!(matches!(
            decode_prefix_bounded(&frame, 8),
            Err(PrefixDecodeError::Limit(_))
        ));
    }
}
