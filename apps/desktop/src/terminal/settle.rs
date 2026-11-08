//! After a cancelled or failed ZMODEM transfer the remote program may still
//! be sending file data for a moment (what was in flight, plus what the
//! transfer had not read yet). Showing it would fill the screen with
//! binary noise, so it is read and dropped until the line goes quiet.
//! What the shell prints once the sender has stopped (its prompt) is kept.

use std::time::Duration;

use hostwick_term::xfer::Link;
use tokio::time::Instant;

/// Quiet for this long: the remote program has stopped.
const QUIET: Duration = Duration::from_millis(250);
/// Never wait longer than this for it to stop.
const LIMIT: Duration = Duration::from_secs(3);

/// A byte that belongs in readable terminal output (or is part of a
/// multi-byte character, or an escape sequence).
fn texty(b: u8) -> bool {
    matches!(b, 0x20..=0x7e | b'\r' | b'\n' | b'\t' | 0x1b | 0x07 | 0x08) || b >= 0x80
}

/// Fold `chunk` into `tail`, the output worth showing: text is kept, and
/// binary data discards everything up to its last byte.
pub fn absorb(tail: &mut Vec<u8>, chunk: &[u8]) {
    let Some(last_bad) = chunk.iter().rposition(|&b| !texty(b)) else {
        tail.extend_from_slice(chunk);
        return;
    };
    tail.clear();
    let rest = &chunk[last_bad + 1..];
    // The rest may start in the middle of a line or a character: begin at
    // the next line or escape sequence.
    let start = rest
        .iter()
        .position(|&b| b == 0x1b || b == b'\r' || b == b'\n')
        .unwrap_or(rest.len());
    tail.extend_from_slice(&rest[start..]);
}

/// Read and drop what the remote program is still sending; returns the
/// text that followed it.
pub async fn settle(link: &mut Link) -> Vec<u8> {
    let mut tail = Vec::new();
    let first = link.leftover();
    absorb(&mut tail, &first);
    let deadline = Instant::now() + LIMIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left.min(QUIET), link.rx.recv()).await {
            Ok(Some(chunk)) => absorb(&mut tail, &chunk),
            Ok(None) | Err(_) => break,
        }
    }
    tail
}

#[cfg(test)]
mod tests {
    use super::absorb;

    #[test]
    fn binary_noise_is_dropped_and_the_prompt_kept() {
        let mut tail = Vec::new();
        absorb(&mut tail, b"\x00\x00@@@\x18\x18\x01\x02\x03 data");
        assert!(tail.is_empty());
        // A chunk that ends with the shell's prompt.
        absorb(
            &mut tail,
            b"\x00\x01\x02garbage\r\n\x1b[32muser@host\x1b[0m:~$ ",
        );
        assert_eq!(tail, b"\r\n\x1b[32muser@host\x1b[0m:~$ ");
        // More noise after it would drop the prompt again.
        absorb(&mut tail, b"\x00\x00\x00");
        assert!(tail.is_empty());
    }

    #[test]
    fn plain_text_is_kept_whole() {
        let mut tail = Vec::new();
        absorb(&mut tail, "sz: transfer cancelled\r\n".as_bytes());
        absorb(&mut tail, "user@host:~$ ".as_bytes());
        absorb(&mut tail, "caf\u{e9} \u{20ac}".as_bytes());
        assert_eq!(
            String::from_utf8_lossy(&tail),
            "sz: transfer cancelled\r\nuser@host:~$ caf\u{e9} \u{20ac}"
        );
    }
}
