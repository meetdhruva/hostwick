//! Writes terminal output to a file (PuTTY "Session logging").

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use alacritty_terminal::vte::{Parser, Perform};
use hostwick_core::LogMode;

/// Collects the text a terminal would show, dropping escape sequences.
#[derive(Default)]
struct TextSink {
    buf: Vec<u8>,
}

impl Perform for TextSink {
    fn print(&mut self, c: char) {
        let mut b = [0u8; 4];
        self.buf.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.buf.push(b'\n'),
            b'\t' => self.buf.push(b'\t'),
            _ => {}
        }
    }
}

pub struct SessionLogger {
    out: BufWriter<File>,
    mode: LogMode,
    parser: Parser,
    sink: TextSink,
    path: PathBuf,
}

impl SessionLogger {
    pub fn open(path: &Path, mode: LogMode, append: bool, header: &str) -> std::io::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)?;
        let mut out = BufWriter::new(file);
        writeln!(out, "=~=~=~= Hostwick log {header} =~=~=~=")?;
        out.flush()?;
        Ok(Self {
            out,
            mode,
            parser: Parser::new(),
            sink: TextSink::default(),
            path: path.to_owned(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self.mode {
            LogMode::Raw => self.out.write_all(bytes)?,
            LogMode::Printable => {
                self.parser.advance(&mut self.sink, bytes);
                self.out.write_all(&self.sink.buf)?;
                self.sink.buf.clear();
            }
        }
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_mode_strips_escapes_raw_keeps_them() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let (p, r) = (dir.path().join("sub/p.log"), dir.path().join("r.log"));
        let input = b"\x1b[1;32mgreen\x1b[0m\r\nnext\tline\x1b]0;title\x07\n";
        let mut lp = SessionLogger::open(&p, LogMode::Printable, false, "test")?;
        lp.write(&input[..9])?; // split mid-sequence on purpose
        lp.write(&input[9..])?;
        let mut lr = SessionLogger::open(&r, LogMode::Raw, false, "test")?;
        lr.write(input)?;
        let text = std::fs::read_to_string(&p)?;
        assert!(text.ends_with("green\nnext\tline\n"), "{text:?}");
        assert!(std::fs::read(&r)?.ends_with(input));
        // Append keeps earlier content.
        SessionLogger::open(&p, LogMode::Printable, true, "again")?.write(b"more\n")?;
        let text = std::fs::read_to_string(&p)?;
        assert!(text.contains("green") && text.ends_with("more\n"));
        Ok(())
    }
}
