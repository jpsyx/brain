//! Bounded diagnostic excerpts shared by the TUI and CLI.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, ensure};

const MAX_BYTES: u64 = 256 * 1024;
const MAX_LINES: usize = 2_000;

/// Read the most recent log context without loading a days-long run into memory.
pub fn recent_logs(path: &Path) -> Result<String> {
    let mut file = File::open(path).context("read brain log file")?;
    ensure!(file.metadata()?.is_file(), "log path is not a regular file");
    let length = file.metadata()?.len();
    ensure!(length > 0, "the log file has no entries yet");
    let start = length.saturating_sub(MAX_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(length - start).read_to_end(&mut bytes)?;
    let mut omitted = start > 0;
    if omitted {
        // A byte-bounded read may start in a UTF-8 code point or a log record.
        if let Some(newline) = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .filter(|index| index + 1 < bytes.len())
        {
            bytes.drain(..=newline);
        } else {
            let continuation = bytes
                .iter()
                .take_while(|byte| **byte & 0xc0 == 0x80)
                .count();
            bytes.drain(..continuation);
        }
    }
    let text = String::from_utf8(bytes).context("log file is not UTF-8 text")?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let first = lines.len().saturating_sub(MAX_LINES);
    omitted |= first > 0;
    let body = lines[first..].concat();
    let notice = if omitted {
        "Older log content omitted; open the source file for the full history."
    } else {
        "Complete log content as read."
    };
    Ok(format!(
        "Brain version: {} (exporter)\nLog source: {}\nRecent log context (up to 2,000 lines / 256 KiB).\n{notice}\n\n{body}",
        env!("CARGO_PKG_VERSION"),
        path.display()
    ))
}
