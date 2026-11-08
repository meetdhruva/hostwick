//! ZMODEM (`rz` / `sz`, as in lrzsz): the file transfer many network
//! engineers run inside an SSH, telnet or serial session.
//!
//! - [`Decoder`] turns the byte stream into headers and data subpackets:
//!   hex, binary-16 and binary-32 headers, ZDLE unescaping, CRC-16 and
//!   CRC-32 checks, five CANs as a cancel.
//! - [`receive`] is the `rz` side (the remote ran `sz`: a download), [`send`]
//!   the `sz` side (the remote ran `rz`: an upload).
//! - [`detect_start`] spots the sequences that start a transfer
//!   (`**ZDLE B00` = ZRQINIT from `sz`, `**ZDLE B01` = ZRINIT from `rz`).
//!
//! Streaming with acknowledgements every few blocks and a bounded window,
//! so a slow link slows the file reader. Resync (ZRPOS) after errors, and
//! cancel with a run of CAN bytes.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::xfer::{
    Link, Outcome, PartFile, Progress, ProgressFn, SendFile, XferError, next_chunk, safe_file_name,
};

pub const ZPAD: u8 = b'*';
pub const ZDLE: u8 = 0x18;
pub const XON: u8 = 0x11;
const CAN: u8 = 0x18;

// Frame types.
pub const ZRQINIT: u8 = 0;
pub const ZRINIT: u8 = 1;
pub const ZSINIT: u8 = 2;
pub const ZACK: u8 = 3;
pub const ZFILE: u8 = 4;
pub const ZSKIP: u8 = 5;
pub const ZNAK: u8 = 6;
pub const ZABORT: u8 = 7;
pub const ZFIN: u8 = 8;
pub const ZRPOS: u8 = 9;
pub const ZDATA: u8 = 10;
pub const ZEOF: u8 = 11;
pub const ZFERR: u8 = 12;
pub const ZCRC: u8 = 13;
pub const ZCHALLENGE: u8 = 14;
pub const ZCOMPL: u8 = 15;
pub const ZCAN: u8 = 16;
pub const ZFREECNT: u8 = 17;
pub const ZCOMMAND: u8 = 18;

// Data subpacket terminators.
pub const ZCRCE: u8 = b'h';
pub const ZCRCG: u8 = b'i';
pub const ZCRCQ: u8 = b'j';
pub const ZCRCW: u8 = b'k';

// ZRINIT capability flags (in the last header byte).
pub const CANFDX: u8 = 0x01;
pub const CANOVIO: u8 = 0x02;
pub const CANFC32: u8 = 0x20;
pub const ESCCTL: u8 = 0x40;

/// Blocks between acknowledgement requests (ZCRCQ).
const ACK_EVERY: usize = 16;
/// Bytes that may be unacknowledged while sending.
const WINDOW: u64 = 256 * 1024;
const BLOCK: usize = 4096;
/// Longest data subpacket the decoder accepts (after unescaping). Real
/// senders use 1 KiB (lrzsz default) up to 8 KiB; anything longer is
/// damage or an attack on memory, and is answered with a resync.
pub const MAX_SUBPACKET: usize = 16 * 1024;
/// How long to wait for the other side before giving up.
const PATIENCE: Duration = Duration::from_secs(30);

// ---- CRC ------------------------------------------------------------------

const fn crc16_table() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u16) << 8;
        let mut k = 0;
        while k < 8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ 0x1021
            } else {
                c << 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

const fn crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC16: [u16; 256] = crc16_table();
static CRC32: [u32; 256] = crc32_table();

/// CRC-16/XMODEM (polynomial 0x1021, start 0) of `data` continuing `crc`.
pub fn crc16_update(mut crc: u16, data: &[u8]) -> u16 {
    for &b in data {
        crc = (crc << 8) ^ CRC16[usize::from((crc >> 8) as u8 ^ b)];
    }
    crc
}

pub fn crc16(data: &[u8]) -> u16 {
    crc16_update(0, data)
}

/// CRC-32 (IEEE) running state; start with `!0`, finish with `!crc`.
pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc = (crc >> 8) ^ CRC32[usize::from((crc as u8) ^ b)];
    }
    crc
}

pub fn crc32(data: &[u8]) -> u32 {
    !crc32_update(!0, data)
}

// ---- encoding -------------------------------------------------------------

/// Append `b`, ZDLE-escaped when the link needs it.
fn put(out: &mut Vec<u8>, b: u8, esc_ctl: bool, last: &mut u8) {
    let must = matches!(b, ZDLE | 0x10 | 0x90 | XON | 0x91 | 0x13 | 0x93)
        || (esc_ctl && (b & 0x60 == 0))
        // CR after '@' would read as the telnet/rlogin escape "@\r@\r".
        || ((b & 0x7f) == 0x0d && (*last & 0x7f) == b'@')
        || (esc_ctl && (b == 0x7f || b == 0xff));
    if must {
        out.push(ZDLE);
        let e = match b {
            0x7f => b'l',
            0xff => b'm',
            _ => b ^ 0x40,
        };
        out.push(e);
        *last = e;
    } else {
        out.push(b);
        *last = b;
    }
}

fn hex_digit(n: u8) -> u8 {
    b"0123456789abcdef"[usize::from(n & 15)]
}

/// A hex header (`**ZDLE B` + 14 hex digits + CR LF [XON]): the form used
/// for everything the receiver says and for ZRQINIT/ZFIN.
pub fn hex_header(kind: u8, arg: [u8; 4]) -> Vec<u8> {
    let mut body = vec![kind];
    body.extend_from_slice(&arg);
    let crc = crc16(&body);
    body.extend_from_slice(&crc.to_be_bytes());
    let mut out = vec![ZPAD, ZPAD, ZDLE, b'B'];
    for b in body {
        out.push(hex_digit(b >> 4));
        out.push(hex_digit(b));
    }
    out.extend_from_slice(b"\r\x8a");
    if kind != ZFIN && kind != ZACK {
        out.push(XON);
    }
    out
}

/// A binary header with a 16- or 32-bit CRC, ZDLE-escaped.
pub fn bin_header(kind: u8, arg: [u8; 4], crc32_mode: bool, esc_ctl: bool) -> Vec<u8> {
    let mut body = vec![kind];
    body.extend_from_slice(&arg);
    let mut out = vec![ZPAD, ZDLE, if crc32_mode { b'C' } else { b'A' }];
    let mut last = 0;
    if crc32_mode {
        let crc = !crc32_update(!0, &body);
        body.extend_from_slice(&crc.to_le_bytes());
    } else {
        let crc = crc16(&body);
        body.extend_from_slice(&crc.to_be_bytes());
    }
    for b in body {
        put(&mut out, b, esc_ctl, &mut last);
    }
    out
}

/// One data subpacket: escaped data, `ZDLE end`, escaped CRC.
pub fn data_subpacket(data: &[u8], end: u8, crc32_mode: bool, esc_ctl: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 8 + 16);
    let mut last = 0;
    for &b in data {
        put(&mut out, b, esc_ctl, &mut last);
    }
    out.push(ZDLE);
    out.push(end);
    if crc32_mode {
        let crc = !crc32_update(crc32_update(!0, data), &[end]);
        for b in crc.to_le_bytes() {
            put(&mut out, b, esc_ctl, &mut last);
        }
    } else {
        let crc = crc16_update(crc16(data), &[end]);
        for b in crc.to_be_bytes() {
            put(&mut out, b, esc_ctl, &mut last);
        }
    }
    if end == ZCRCW {
        out.push(XON);
    }
    out
}

/// The cancel sequence: eight CAN and eight backspaces (the backspaces
/// erase the CANs if the other side already stopped).
pub fn cancel_sequence() -> Vec<u8> {
    let mut v = vec![CAN; 8];
    v.extend_from_slice(&[0x08; 8]);
    v
}

fn pos_arg(pos: u32) -> [u8; 4] {
    pos.to_le_bytes()
}

// ---- decoding -------------------------------------------------------------

/// A parsed header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub kind: u8,
    pub arg: [u8; 4],
    /// Binary-32 header: the data subpackets that follow use CRC-32.
    pub crc32: bool,
}

impl Header {
    /// The position (ZRPOS, ZEOF, ZDATA, ZACK), little endian.
    pub fn pos(&self) -> u32 {
        u32::from_le_bytes(self.arg)
    }

    /// ZRINIT capability flags.
    pub fn flags(&self) -> u8 {
        self.arg[3]
    }

    /// Headers that are followed by a data subpacket.
    fn has_data(&self) -> bool {
        matches!(self.kind, ZFILE | ZSINIT | ZDATA | ZCOMMAND)
    }
}

/// What the decoder found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Header(Header),
    /// A data subpacket whose CRC matched; `end` is its terminator.
    Data {
        data: Vec<u8>,
        end: u8,
    },
    /// A data subpacket that was damaged.
    DataError,
    /// Five CANs in a row: the other side cancelled.
    Cancelled,
}

#[derive(Debug)]
enum St {
    Hunt,
    Pad,
    Zdle,
    Hex(Vec<u8>),
    Bin {
        crc32: bool,
        buf: Vec<u8>,
        esc: bool,
    },
    Data {
        crc32: bool,
        buf: Vec<u8>,
        esc: bool,
    },
    DataCrc {
        crc32: bool,
        data: Vec<u8>,
        end: u8,
        crc: Vec<u8>,
        esc: bool,
    },
}

/// Incremental parser for the receiving side of a ZMODEM link.
#[derive(Debug)]
pub struct Decoder {
    st: St,
    cans: u8,
    /// Printable text seen between frames (error messages of `sz`/`rz`).
    noise: Vec<u8>,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

fn unhex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The byte after ZDLE, decoded; `Err(end)` for a frame terminator.
enum Unesc {
    Byte(u8),
    End(u8),
    Bad,
}

fn unescape(b: u8) -> Unesc {
    match b {
        ZCRCE | ZCRCG | ZCRCQ | ZCRCW => Unesc::End(b),
        b'l' => Unesc::Byte(0x7f),
        b'm' => Unesc::Byte(0xff),
        _ if b & 0x60 == 0x40 => Unesc::Byte(b ^ 0x40),
        _ => Unesc::Bad,
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            st: St::Hunt,
            cans: 0,
            noise: Vec::new(),
        }
    }

    /// Bytes held for the subpacket being collected (tests check the cap).
    #[cfg(test)]
    fn buffered(&self) -> usize {
        match &self.st {
            St::Data { buf, .. } => buf.len(),
            St::DataCrc { data, .. } => data.len(),
            _ => 0,
        }
    }

    /// Text that was not part of any frame (up to 4 KB); taken once.
    pub fn take_noise(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.noise)
    }

    /// Feed bytes; parsed items are appended to `out`.
    pub fn push(&mut self, bytes: &[u8], out: &mut VecDeque<Item>) {
        for &b in bytes {
            if b == CAN {
                self.cans += 1;
                if self.cans >= 5 {
                    self.cans = 0;
                    self.st = St::Hunt;
                    out.push_back(Item::Cancelled);
                    continue;
                }
            } else {
                self.cans = 0;
            }
            self.byte(b, out);
        }
    }

    fn hunt_noise(&mut self, b: u8) {
        // Everything but the flow-control and cancel bytes is kept: it is
        // terminal output (error text, and after "OO" the next prompt with
        // its escape sequences and UTF-8).
        if !matches!(b, CAN | XON | 0x13 | 0x91 | 0x93) && self.noise.len() < 64 * 1024 {
            self.noise.push(b);
        }
    }

    fn byte(&mut self, b: u8, out: &mut VecDeque<Item>) {
        let st = std::mem::replace(&mut self.st, St::Hunt);
        self.st = self.step(st, b, out);
    }

    fn step(&mut self, st: St, b: u8, out: &mut VecDeque<Item>) -> St {
        let flow = matches!(b, XON | 0x13 | 0x91 | 0x93);
        match st {
            St::Hunt => {
                if b == ZPAD {
                    St::Pad
                } else {
                    self.hunt_noise(b);
                    St::Hunt
                }
            }
            St::Pad => match b {
                ZPAD => St::Pad,
                ZDLE => St::Zdle,
                _ => {
                    self.hunt_noise(b);
                    St::Hunt
                }
            },
            St::Zdle => match b {
                b'B' => St::Hex(Vec::with_capacity(14)),
                b'A' => St::Bin {
                    crc32: false,
                    buf: Vec::with_capacity(7),
                    esc: false,
                },
                b'C' => St::Bin {
                    crc32: true,
                    buf: Vec::with_capacity(9),
                    esc: false,
                },
                ZPAD => St::Pad,
                _ => St::Hunt,
            },
            St::Hex(mut buf) => {
                if let Some(v) = unhex(b) {
                    buf.push(v);
                    if buf.len() < 14 {
                        return St::Hex(buf);
                    }
                    let bytes: Vec<u8> = buf.chunks(2).map(|c| c[0] << 4 | c[1]).collect();
                    let crc = u16::from_be_bytes([bytes[5], bytes[6]]);
                    if crc16(&bytes[..5]) == crc {
                        let h = Header {
                            kind: bytes[0],
                            arg: [bytes[1], bytes[2], bytes[3], bytes[4]],
                            crc32: false,
                        };
                        self.after_header(h, out)
                    } else {
                        St::Hunt
                    }
                } else if b == ZPAD {
                    St::Pad
                } else {
                    St::Hunt
                }
            }
            St::Bin {
                crc32,
                mut buf,
                esc,
            } => {
                if esc {
                    match unescape(b) {
                        Unesc::Byte(v) => buf.push(v),
                        _ => return St::Hunt,
                    }
                } else if b == ZDLE {
                    return St::Bin {
                        crc32,
                        buf,
                        esc: true,
                    };
                } else if flow {
                    return St::Bin {
                        crc32,
                        buf,
                        esc: false,
                    };
                } else {
                    buf.push(b);
                }
                let want = if crc32 { 9 } else { 7 };
                if buf.len() < want {
                    return St::Bin {
                        crc32,
                        buf,
                        esc: false,
                    };
                }
                let ok = if crc32 {
                    let crc = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
                    !crc32_update(!0, &buf[..5]) == crc
                } else {
                    crc16(&buf[..5]) == u16::from_be_bytes([buf[5], buf[6]])
                };
                if !ok {
                    return St::Hunt;
                }
                let h = Header {
                    kind: buf[0],
                    arg: [buf[1], buf[2], buf[3], buf[4]],
                    crc32,
                };
                self.after_header(h, out)
            }
            St::Data {
                crc32,
                mut buf,
                esc,
            } => {
                // No terminator after this many bytes: damage, or a peer
                // trying to make us buffer without limit.
                if buf.len() > MAX_SUBPACKET {
                    out.push_back(Item::DataError);
                    return St::Hunt;
                }
                if esc {
                    match unescape(b) {
                        Unesc::Byte(v) => {
                            buf.push(v);
                            St::Data {
                                crc32,
                                buf,
                                esc: false,
                            }
                        }
                        Unesc::End(end) => St::DataCrc {
                            crc32,
                            data: buf,
                            end,
                            crc: Vec::new(),
                            esc: false,
                        },
                        Unesc::Bad => {
                            out.push_back(Item::DataError);
                            St::Hunt
                        }
                    }
                } else if b == ZDLE {
                    St::Data {
                        crc32,
                        buf,
                        esc: true,
                    }
                } else if flow {
                    St::Data {
                        crc32,
                        buf,
                        esc: false,
                    }
                } else {
                    buf.push(b);
                    St::Data {
                        crc32,
                        buf,
                        esc: false,
                    }
                }
            }
            St::DataCrc {
                crc32,
                data,
                end,
                mut crc,
                esc,
            } => {
                if esc {
                    match unescape(b) {
                        Unesc::Byte(v) => crc.push(v),
                        _ => {
                            out.push_back(Item::DataError);
                            return St::Hunt;
                        }
                    }
                } else if b == ZDLE {
                    return St::DataCrc {
                        crc32,
                        data,
                        end,
                        crc,
                        esc: true,
                    };
                } else if flow {
                    return St::DataCrc {
                        crc32,
                        data,
                        end,
                        crc,
                        esc: false,
                    };
                } else {
                    crc.push(b);
                }
                let need = if crc32 { 4 } else { 2 };
                if crc.len() < need {
                    return St::DataCrc {
                        crc32,
                        data,
                        end,
                        crc,
                        esc: false,
                    };
                }
                let ok = if crc32 {
                    let got = u32::from_le_bytes([crc[0], crc[1], crc[2], crc[3]]);
                    !crc32_update(crc32_update(!0, &data), &[end]) == got
                } else {
                    let got = u16::from_be_bytes([crc[0], crc[1]]);
                    crc16_update(crc16(&data), &[end]) == got
                };
                if !ok {
                    out.push_back(Item::DataError);
                    return St::Hunt;
                }
                let more = matches!(end, ZCRCG | ZCRCQ);
                out.push_back(Item::Data { data, end });
                if more {
                    St::Data {
                        crc32,
                        buf: Vec::new(),
                        esc: false,
                    }
                } else {
                    St::Hunt
                }
            }
        }
    }

    /// Push the header and pick the next state (data follows some headers).
    fn after_header(&mut self, h: Header, out: &mut VecDeque<Item>) -> St {
        let next = if h.has_data() {
            St::Data {
                crc32: h.crc32,
                buf: Vec::new(),
                esc: false,
            }
        } else {
            St::Hunt
        };
        out.push_back(Item::Header(h));
        next
    }
}

// ---- detecting a transfer start ------------------------------------------

/// Which side the remote program plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// `sz` is sending (ZRQINIT): this is a download.
    Download,
    /// `rz` is waiting (ZRINIT): this is an upload.
    Upload,
}

/// `**` ZDLE `B0`: the start of a hex header of type 00 or 01.
const SIG: [u8; 5] = [ZPAD, ZPAD, ZDLE, b'B', b'0'];

/// Find the start of a transfer in `buf`: its offset and kind.
pub fn find_start(buf: &[u8]) -> Option<(usize, Start)> {
    let mut from = 0;
    while let Some(i) = buf[from..]
        .windows(SIG.len() + 1)
        .position(|w| w[..SIG.len()] == SIG)
    {
        let at = from + i;
        match buf[at + SIG.len()] {
            b'0' => return Some((at, Start::Download)),
            b'1' => return Some((at, Start::Upload)),
            _ => from = at + 1,
        }
    }
    None
}

/// How many bytes at the end of `buf` could be the beginning of a start
/// sequence that is not complete yet (from a single `*` on; the terminal
/// shows held bytes after a moment if nothing follows).
pub fn partial_start_len(buf: &[u8]) -> usize {
    (1..=SIG.len())
        .rev()
        .find(|&k| buf.len() >= k && buf.ends_with(&SIG[..k]))
        .unwrap_or(0)
}

// ---- the protocol -----------------------------------------------------------

struct Z<'a> {
    link: &'a mut Link,
    dec: Decoder,
    q: VecDeque<Item>,
    cancel: &'a CancellationToken,
}

impl<'a> Z<'a> {
    fn new(link: &'a mut Link, cancel: &'a CancellationToken, initial: &[u8]) -> Self {
        let mut z = Self {
            link,
            dec: Decoder::new(),
            q: VecDeque::new(),
            cancel,
        };
        z.dec.push(initial, &mut z.q);
        z
    }

    async fn next(&mut self, timeout: Duration) -> Result<Item, XferError> {
        loop {
            if let Some(i) = self.q.pop_front() {
                return match i {
                    Item::Cancelled => Err(XferError::RemoteCancelled),
                    i => Ok(i),
                };
            }
            let chunk = next_chunk(self.link, self.cancel, timeout).await?;
            self.dec.push(&chunk, &mut self.q);
        }
    }

    /// The next item that is already here, without waiting.
    fn poll(&mut self) -> Result<Option<Item>, XferError> {
        loop {
            if let Some(i) = self.q.pop_front() {
                return match i {
                    Item::Cancelled => Err(XferError::RemoteCancelled),
                    i => Ok(Some(i)),
                };
            }
            match self.link.rx.try_recv() {
                Ok(chunk) => self.dec.push(&chunk, &mut self.q),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => return Ok(None),
                Err(_) => return Err(XferError::Closed),
            }
        }
    }

    async fn send(&self, bytes: Vec<u8>) -> Result<(), XferError> {
        if self.cancel.is_cancelled() {
            return Err(XferError::Cancelled);
        }
        self.link.send(bytes).await
    }

    /// Cancel on the wire (unless the other side is the one that ended)
    /// and keep text that was not part of the protocol for the terminal.
    async fn finish_err(self, err: XferError) -> XferError {
        if matches!(
            err,
            XferError::Cancelled | XferError::Timeout | XferError::Protocol(_) | XferError::Io(_)
        ) {
            let _ = self.link.send(cancel_sequence()).await;
        }
        self.link.spare = self.dec.noise;
        err
    }
}

/// ZMODEM positions are 32 bits wide.
fn fits(size: u64, name: &str) -> Result<(), XferError> {
    if size > u64::from(u32::MAX) {
        return Err(XferError::Protocol(format!(
            "{name} is larger than 4 GB, which ZMODEM cannot address"
        )));
    }
    Ok(())
}

struct RxFile {
    /// Written as `name.part`, renamed to its final name at ZEOF; deleted
    /// when dropped (cancel, error, a new ZFILE mid-file).
    file: PartFile,
    name: String,
    size: u64,
    pos: u64,
    mtime: u64,
    index: usize,
}

/// Parse a ZFILE subpacket: `name NUL size mtime(octal) mode serial ...`.
pub fn parse_file_info(data: &[u8]) -> Option<(String, u64, u64, u32)> {
    let nul = data.iter().position(|&b| b == 0)?;
    let name = String::from_utf8_lossy(&data[..nul]).into_owned();
    let rest = &data[nul + 1..];
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    let rest = String::from_utf8_lossy(&rest[..end]).into_owned();
    let mut it = rest.split_whitespace();
    let size = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mtime = it
        .next()
        .and_then(|s| u64::from_str_radix(s, 8).ok())
        .unwrap_or(0);
    let mode = it
        .next()
        .and_then(|s| u32::from_str_radix(s, 8).ok())
        .unwrap_or(0o644);
    Some((name, size, mtime, mode))
}

/// Receive files from a remote `sz` into `dest`. `initial` holds the bytes
/// from the start sequence on. Data goes to a `.part` file that gets its
/// real name only when the file ended cleanly (ZEOF at the right position);
/// a cancelled or failed transfer leaves nothing behind. Existing files are
/// never overwritten (the new one gets ` (1)`…).
pub async fn receive(
    link: &mut Link,
    initial: &[u8],
    dest: &std::path::Path,
    cancel: &CancellationToken,
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let mut z = Z::new(link, cancel, initial);
    match receive_inner(&mut z, dest, progress).await {
        Ok(o) => Ok(o),
        Err(e) => Err(z.finish_err(e).await),
    }
}

async fn receive_inner(
    z: &mut Z<'_>,
    dest: &std::path::Path,
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let zrinit = || hex_header(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32 | ESCCTL]);
    let mut out = Outcome::default();
    let mut cur: Option<RxFile> = None;
    let mut errors = 0u32;
    let mut idle = 0u32;
    let mut index = 0usize;
    if z.q.is_empty() {
        z.send(zrinit()).await?;
    }
    loop {
        let item = match z.next(Duration::from_secs(10)).await {
            Ok(i) => {
                idle = 0;
                i
            }
            Err(XferError::Timeout) => {
                idle += 1;
                if idle > 3 {
                    return Err(XferError::Timeout);
                }
                let again = match &cur {
                    Some(f) => hex_header(ZRPOS, pos_arg(f.pos as u32)),
                    None => zrinit(),
                };
                z.send(again).await?;
                continue;
            }
            Err(e) => return Err(e),
        };
        let h = match item {
            Item::Header(h) => h,
            // Data without a header (after an error) or noise.
            _ => continue,
        };
        match h.kind {
            ZRQINIT => z.send(zrinit()).await?,
            ZSINIT => {
                // Attention string: read and acknowledge.
                let _ = z.next(PATIENCE).await?;
                z.send(hex_header(ZACK, pos_arg(1))).await?;
            }
            ZCHALLENGE => z.send(hex_header(ZACK, h.arg)).await?,
            ZCOMMAND => {
                // Running commands for the sender is never allowed.
                let _ = z.next(PATIENCE).await?;
                z.send(hex_header(ZCOMPL, pos_arg(1))).await?;
            }
            ZFILE => {
                let data = match z.next(PATIENCE).await? {
                    Item::Data { data, .. } => data,
                    _ => {
                        z.send(hex_header(ZNAK, [0; 4])).await?;
                        continue;
                    }
                };
                // A new file before the old one ended: the old one is cut.
                cur = None;
                let Some((raw, size, mtime, _mode)) = parse_file_info(&data) else {
                    z.send(hex_header(ZNAK, [0; 4])).await?;
                    continue;
                };
                let Some(name) = safe_file_name(&raw) else {
                    z.send(hex_header(ZSKIP, [0; 4])).await?;
                    continue;
                };
                let file = match PartFile::create(dest, &name) {
                    Ok(f) => f,
                    Err(e) => return Err(XferError::Io(format!("{}: {e}", dest.display()))),
                };
                index += 1;
                progress(&Progress {
                    name: name.clone(),
                    done: 0,
                    total: size,
                    index,
                    count: 0,
                });
                cur = Some(RxFile {
                    file,
                    name,
                    size,
                    pos: 0,
                    mtime,
                    index,
                });
                z.send(hex_header(ZRPOS, pos_arg(0))).await?;
            }
            ZDATA => {
                let Some(f) = cur.as_mut() else {
                    z.send(hex_header(ZRPOS, pos_arg(0))).await?;
                    continue;
                };
                if u64::from(h.pos()) != f.pos {
                    z.send(hex_header(ZRPOS, pos_arg(f.pos as u32))).await?;
                    continue;
                }
                loop {
                    match z.next(PATIENCE).await? {
                        Item::Data { data, end } => {
                            f.file.file().write_all(&data).await?;
                            f.pos += data.len() as u64;
                            out.bytes += data.len() as u64;
                            progress(&Progress {
                                name: f.name.clone(),
                                done: f.pos,
                                total: f.size.max(f.pos),
                                index: f.index,
                                count: 0,
                            });
                            if matches!(end, ZCRCQ | ZCRCW) {
                                z.send(hex_header(ZACK, pos_arg(f.pos as u32))).await?;
                            }
                            if matches!(end, ZCRCE | ZCRCW) {
                                break;
                            }
                        }
                        Item::DataError => {
                            errors += 1;
                            if errors > 30 {
                                return Err(XferError::Protocol(
                                    "too many transmission errors".into(),
                                ));
                            }
                            z.send(hex_header(ZRPOS, pos_arg(f.pos as u32))).await?;
                            break;
                        }
                        Item::Header(other) => {
                            z.q.push_front(Item::Header(other));
                            break;
                        }
                        Item::Cancelled => return Err(XferError::RemoteCancelled),
                    }
                }
            }
            ZEOF => {
                let Some(f) = cur.as_mut() else {
                    z.send(zrinit()).await?;
                    continue;
                };
                if u64::from(h.pos()) != f.pos {
                    z.send(hex_header(ZRPOS, pos_arg(f.pos as u32))).await?;
                    continue;
                }
                let Some(f) = cur.take() else { continue };
                let mtime =
                    (f.mtime > 0).then(|| std::time::UNIX_EPOCH + Duration::from_secs(f.mtime));
                let path = f
                    .file
                    .commit(mtime)
                    .await
                    .map_err(|e| XferError::Io(format!("{}: {e}", f.name)))?;
                out.files.push(path);
                z.send(zrinit()).await?;
            }
            ZFIN => {
                z.send(hex_header(ZFIN, [0; 4])).await?;
                let _ = z.dec.take_noise();
                // The sender says "OO" and leaves.
                let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
                loop {
                    if z.dec.noise.iter().filter(|&&b| b == b'O').count() >= 2 {
                        break;
                    }
                    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    match next_chunk(z.link, z.cancel, left).await {
                        Ok(c) => z.dec.push(&c, &mut z.q),
                        Err(_) => break,
                    }
                }
                let noise = z.dec.take_noise();
                // What follows "OO" is the shell's next output, raw.
                let rest = match noise.windows(2).position(|w| w == b"OO") {
                    Some(i) => &noise[i + 2..],
                    None => &noise[..],
                };
                z.link.spare = rest.to_vec();
                return Ok(out);
            }
            ZABORT | ZCAN | ZFERR => return Err(XferError::RemoteCancelled),
            _ => {}
        }
    }
}

async fn read_block(file: &mut tokio::fs::File, blk: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; blk];
    let mut n = 0;
    while n < blk {
        let r = file.read(&mut buf[n..]).await?;
        if r == 0 {
            break;
        }
        n += r;
    }
    buf.truncate(n);
    Ok(buf)
}

/// How a pass over the file data ended.
enum StreamEnd {
    /// ZEOF was sent.
    Eof,
    /// The receiver asked to continue from this position.
    Resync(u64),
}

/// Send the files to a remote `rz`. `initial` holds the bytes from the
/// start sequence on (its ZRINIT).
pub async fn send(
    link: &mut Link,
    initial: &[u8],
    files: &[SendFile],
    cancel: &CancellationToken,
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let mut z = Z::new(link, cancel, initial);
    match send_inner(&mut z, files, progress).await {
        Ok(o) => Ok(o),
        Err(e) => Err(z.finish_err(e).await),
    }
}

async fn send_inner(
    z: &mut Z<'_>,
    files: &[SendFile],
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let mut out = Outcome::default();
    // The receiver's ZRINIT says what it can do.
    let rinit = loop {
        match z.next(PATIENCE).await? {
            Item::Header(h) if h.kind == ZRINIT => break h,
            Item::Header(h) if h.kind == ZCHALLENGE => {
                z.send(hex_header(ZACK, h.arg)).await?;
            }
            Item::Header(h) if matches!(h.kind, ZABORT | ZCAN | ZFERR) => {
                return Err(XferError::RemoteCancelled);
            }
            _ => {}
        }
    };
    let crc32_mode = rinit.flags() & CANFC32 != 0;
    let esc = rinit.flags() & ESCCTL != 0;
    let buf_size = usize::from(u16::from_le_bytes([rinit.arg[0], rinit.arg[1]]));
    let blk = if buf_size == 0 {
        BLOCK
    } else {
        buf_size.clamp(256, BLOCK)
    };
    let mut left = 0u64;
    for f in files {
        left += tokio::fs::metadata(&f.path).await?.len();
    }
    for (ix, f) in files.iter().enumerate() {
        let meta = tokio::fs::metadata(&f.path).await?;
        let size = meta.len();
        fits(size, &f.name)?;
        let mut file = tokio::fs::File::open(&f.path).await?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let info = format!(
            "{}\0{} {:o} 100644 0 {} {}\0",
            f.name,
            size,
            mtime,
            files.len() - ix,
            left
        );
        let report = |progress: &mut (dyn FnMut(&Progress) + Send), done: u64| {
            progress(&Progress {
                name: f.name.clone(),
                done,
                total: size,
                index: ix + 1,
                count: files.len(),
            });
        };
        report(progress, 0);
        // Offer the file; the receiver answers with where to start (or skip).
        let mut tries = 0;
        let mut start = loop {
            z.send(bin_header(ZFILE, [0, 0, 0, 1], crc32_mode, esc))
                .await?;
            z.send(data_subpacket(info.as_bytes(), ZCRCW, crc32_mode, esc))
                .await?;
            let mut offered = None;
            while let Item::Header(h) = z.next(PATIENCE).await? {
                match h.kind {
                    ZRPOS => {
                        offered = Some(Some(u64::from(h.pos())));
                        break;
                    }
                    ZSKIP => {
                        offered = Some(None);
                        break;
                    }
                    ZCRC => {
                        // The receiver wants the CRC of the first N bytes.
                        let n = u64::from(h.pos()).min(size);
                        let mut check = tokio::fs::File::open(&f.path).await?;
                        let mut crc = !0u32;
                        let mut remaining = n;
                        let mut b = vec![0u8; 32 * 1024];
                        while remaining > 0 {
                            let want = b.len().min(remaining as usize);
                            let r = check.read(&mut b[..want]).await?;
                            if r == 0 {
                                break;
                            }
                            crc = crc32_update(crc, &b[..r]);
                            remaining -= r as u64;
                        }
                        z.send(hex_header(ZCRC, (!crc).to_le_bytes())).await?;
                    }
                    ZNAK => break,
                    ZABORT | ZCAN | ZFERR => return Err(XferError::RemoteCancelled),
                    _ => {}
                }
            }
            if let Some(o) = offered {
                break o;
            }
            tries += 1;
            if tries >= 5 {
                return Err(XferError::Protocol(
                    "the receiver did not accept the file".into(),
                ));
            }
        };
        let mut done = false;
        let mut resyncs = 0;
        while let Some(pos) = start {
            if pos > size {
                return Err(XferError::Protocol(
                    "the receiver asked for a position past the end".into(),
                ));
            }
            file.seek(std::io::SeekFrom::Start(pos)).await?;
            let end = stream(z, &mut file, pos, size, blk, crc32_mode, esc, &mut |d| {
                report(progress, d)
            })
            .await?;
            if let StreamEnd::Resync(p) = end {
                resyncs += 1;
                if resyncs > 100 {
                    return Err(XferError::Protocol("too many transmission errors".into()));
                }
                start = Some(p);
                continue;
            }
            // After ZEOF: ZRINIT = got it, ZRPOS = resend from there.
            start = loop {
                if let Item::Header(h) = z.next(PATIENCE).await? {
                    match h.kind {
                        ZRINIT => {
                            done = true;
                            break None;
                        }
                        ZRPOS => break Some(u64::from(h.pos())),
                        ZSKIP => break None,
                        ZABORT | ZCAN | ZFERR => return Err(XferError::RemoteCancelled),
                        _ => {}
                    }
                }
            };
        }
        if done {
            out.files.push(f.path.clone());
            out.bytes += size;
            report(progress, size);
        }
        left = left.saturating_sub(size);
    }
    // Say goodbye.
    z.send(hex_header(ZFIN, [0; 4])).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let wait = deadline.saturating_duration_since(tokio::time::Instant::now());
        if wait.is_zero() {
            break;
        }
        match z.next(wait).await {
            Ok(Item::Header(h)) if h.kind == ZFIN => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    z.send(b"OO".to_vec()).await?;
    Ok(out)
}

/// Send the data of `file` from `pos` to the end as one ZDATA frame, with
/// acknowledgement requests and a bounded window; ends with ZEOF.
#[allow(clippy::too_many_arguments)]
async fn stream(
    z: &mut Z<'_>,
    file: &mut tokio::fs::File,
    start: u64,
    size: u64,
    blk: usize,
    crc32_mode: bool,
    esc: bool,
    on_pos: &mut (dyn FnMut(u64) + Send),
) -> Result<StreamEnd, XferError> {
    let mut pos = start;
    let mut acked = start;
    z.send(bin_header(ZDATA, pos_arg(pos as u32), crc32_mode, esc))
        .await?;
    let mut cur = read_block(file, blk).await?;
    let mut count = 0usize;
    loop {
        let next = if cur.is_empty() {
            Vec::new()
        } else {
            read_block(file, blk).await?
        };
        let last = next.is_empty();
        count += 1;
        let end = if last {
            ZCRCE
        } else if count.is_multiple_of(ACK_EVERY) {
            ZCRCQ
        } else {
            ZCRCG
        };
        z.send(data_subpacket(&cur, end, crc32_mode, esc)).await?;
        pos += cur.len() as u64;
        on_pos(pos);
        if last {
            break;
        }
        cur = next;
        // Anything the receiver said meanwhile: acks, or a resync.
        while let Some(item) = z.poll()? {
            if let Item::Header(h) = item {
                match h.kind {
                    ZRPOS => return Ok(StreamEnd::Resync(u64::from(h.pos()))),
                    ZACK => acked = u64::from(h.pos()),
                    ZABORT | ZCAN | ZFERR => return Err(XferError::RemoteCancelled),
                    _ => {}
                }
            }
        }
        while pos.saturating_sub(acked) > WINDOW {
            if let Item::Header(h) = z.next(PATIENCE).await? {
                match h.kind {
                    ZRPOS => return Ok(StreamEnd::Resync(u64::from(h.pos()))),
                    ZACK => acked = u64::from(h.pos()),
                    ZABORT | ZCAN | ZFERR => return Err(XferError::RemoteCancelled),
                    _ => {}
                }
            }
        }
    }
    z.send(bin_header(ZEOF, pos_arg(size as u32), crc32_mode, esc))
        .await?;
    Ok(StreamEnd::Eof)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xfer::testutil::{loopback, spawn_link};

    fn decode_all(bytes: &[u8]) -> Vec<Item> {
        let mut d = Decoder::new();
        let mut q = VecDeque::new();
        d.push(bytes, &mut q);
        q.into_iter().collect()
    }

    fn sample(n: usize) -> Vec<u8> {
        // Every byte value, including the ones that need escaping, in a
        // pseudo-random order.
        let mut x = 0x1234_5678u32;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if i % 7 == 0 {
                    (i % 256) as u8
                } else {
                    (x >> 24) as u8
                }
            })
            .collect()
    }

    #[test]
    fn crc_check_values() {
        assert_eq!(crc16(b"123456789"), 0x31c3);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc16(b""), 0);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn hex_header_matches_lrzsz() {
        // What `sz` prints to start: ZRQINIT with position 0.
        let h = hex_header(ZRQINIT, [0; 4]);
        assert_eq!(&h[..4], b"**\x18B");
        assert_eq!(&h[4..18], b"00000000000000");
        assert_eq!(&h[18..], b"\r\x8a\x11");
        // ZRINIT from rz with flags 0x23: CRC16 of 01 00 00 00 23 is 0xbe50
        // (the well-known "**\x18B0100000023be50").
        let h = hex_header(ZRINIT, [0, 0, 0, 0x23]);
        assert_eq!(&h[4..18], b"0100000023be50");
        // ZFIN and ZACK carry no XON.
        assert_eq!(hex_header(ZFIN, [0; 4]).last(), Some(&0x8a));
        assert_eq!(hex_header(ZACK, [0; 4]).last(), Some(&0x8a));
    }

    #[test]
    fn headers_round_trip_in_every_form() {
        let args = [
            [1, 2, 3, 4],
            [0x18, 0x11, 0x13, 0x90],
            [0xff, 0x7f, 0x0d, 0x40],
        ];
        for arg in args {
            let items = decode_all(&hex_header(ZRPOS, arg));
            assert_eq!(
                items,
                vec![Item::Header(Header {
                    kind: ZRPOS,
                    arg,
                    crc32: false
                })]
            );
            for crc32_mode in [false, true] {
                for esc in [false, true] {
                    let items = decode_all(&bin_header(ZEOF, arg, crc32_mode, esc));
                    assert_eq!(
                        items,
                        vec![Item::Header(Header {
                            kind: ZEOF,
                            arg,
                            crc32: crc32_mode
                        })],
                        "crc32={crc32_mode} esc={esc} arg={arg:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn damaged_headers_are_dropped() {
        let mut h = hex_header(ZRPOS, [1, 2, 3, 4]);
        h[6] = b'f';
        assert!(decode_all(&h).is_empty());
        let mut b = bin_header(ZRPOS, [1, 2, 3, 4], true, false);
        let n = b.len();
        b[n - 1] ^= 0x55;
        assert!(decode_all(&b).is_empty());
    }

    #[test]
    fn data_subpackets_round_trip() {
        let data = sample(5000);
        for crc32_mode in [false, true] {
            for esc in [false, true] {
                for end in [ZCRCE, ZCRCG, ZCRCQ, ZCRCW] {
                    let mut wire = bin_header(ZDATA, pos_arg(0), crc32_mode, esc);
                    wire.extend(data_subpacket(&data, end, crc32_mode, esc));
                    let items = decode_all(&wire);
                    assert_eq!(items.len(), 2, "crc32={crc32_mode} esc={esc} end={end}");
                    assert_eq!(
                        items[1],
                        Item::Data {
                            data: data.clone(),
                            end
                        },
                        "crc32={crc32_mode} esc={esc} end={end}"
                    );
                }
            }
        }
        // Escaping keeps the dangerous bytes off the wire.
        let wire = data_subpacket(
            &[0x18, 0x11, 0x13, 0x91, 0x93, 0x90, 0x10],
            ZCRCE,
            true,
            false,
        );
        assert!(!wire.windows(2).any(|w| w == [ZDLE, ZDLE]));
        assert!(!wire[..wire.len() - 8].contains(&XON));
        let ctl = data_subpacket(&[0, 1, 2, 0x80, 0x7f, 0xff], ZCRCE, false, true);
        assert!(
            ctl[..ctl.len() - 5]
                .iter()
                .all(|b| b & 0x60 != 0 || *b == ZDLE)
        );
    }

    #[test]
    fn streaming_subpackets_follow_each_other() {
        let a = sample(1000);
        let b = sample(300);
        let mut wire = bin_header(ZDATA, pos_arg(10), true, false);
        wire.extend(data_subpacket(&a, ZCRCG, true, false));
        wire.extend(data_subpacket(&b, ZCRCE, true, false));
        wire.extend(bin_header(ZEOF, pos_arg(1310), true, false));
        let items = decode_all(&wire);
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[1],
            Item::Data {
                data: a,
                end: ZCRCG
            }
        );
        assert_eq!(
            items[2],
            Item::Data {
                data: b,
                end: ZCRCE
            }
        );
        assert!(matches!(&items[3], Item::Header(h) if h.kind == ZEOF && h.pos() == 1310));
    }

    #[test]
    fn corrupted_data_is_reported_and_decoding_recovers() {
        let data = sample(2000);
        let mut wire = bin_header(ZDATA, pos_arg(0), false, false);
        let mut sub = data_subpacket(&data, ZCRCE, false, false);
        sub[500] ^= 0x20;
        wire.extend(sub);
        wire.extend(hex_header(ZEOF, pos_arg(2000)));
        let items = decode_all(&wire);
        assert!(items.contains(&Item::DataError), "{items:?}");
        assert!(matches!(items.last(), Some(Item::Header(h)) if h.kind == ZEOF));
    }

    #[test]
    fn five_cans_cancel_and_chunking_does_not_matter() {
        assert_eq!(decode_all(&[CAN; 5]), vec![Item::Cancelled]);
        assert_eq!(decode_all(&[CAN; 4]), vec![]);
        let mut wire = b"junk rz\r".to_vec();
        wire.extend(hex_header(ZRQINIT, [0; 4]));
        wire.extend(bin_header(ZFILE, [0, 0, 0, 1], true, true));
        wire.extend(data_subpacket(
            b"a.bin\0 5 0 100644 0 1 5\0",
            ZCRCW,
            true,
            true,
        ));
        let whole = decode_all(&wire);
        assert_eq!(whole.len(), 3);
        // One byte at a time gives the same result.
        let mut d = Decoder::new();
        let mut q = VecDeque::new();
        for b in &wire {
            d.push(&[*b], &mut q);
        }
        assert_eq!(q.into_iter().collect::<Vec<_>>(), whole);
        // The text before the frames is kept for the terminal.
        let mut d = Decoder::new();
        d.push(&wire, &mut VecDeque::new());
        assert!(String::from_utf8_lossy(&d.take_noise()).contains("junk rz"));
    }

    #[test]
    fn output_after_the_transfer_keeps_escapes_and_utf8() {
        // The shell prompt that follows "OO" must reach the terminal intact.
        let prompt = "OO\x1b[?2004h\x1b]0;a\u{7}\x1b[32m\u{20ac}\x1b[0m$ ";
        let mut d = Decoder::new();
        d.push(prompt.as_bytes(), &mut VecDeque::new());
        assert_eq!(d.take_noise(), prompt.as_bytes());
    }

    #[test]
    fn start_sequences() {
        let sz = [b"rz\r".to_vec(), hex_header(ZRQINIT, [0; 4])].concat();
        assert_eq!(find_start(&sz), Some((3, Start::Download)));
        let rz = [b"waiting".to_vec(), hex_header(ZRINIT, [0, 0, 0, 0x23])].concat();
        assert_eq!(find_start(&rz), Some((7, Start::Upload)));
        assert_eq!(find_start(b"**\x18B0"), None);
        assert_eq!(find_start(b"**\x18B02"), None);
        assert_eq!(find_start(b"plain ** text"), None);
        assert_eq!(partial_start_len(b"x**\x18"), 3);
        assert_eq!(partial_start_len(b"x*"), 1);
        assert_eq!(partial_start_len(b"x**"), 2);
        assert_eq!(partial_start_len(b"abc"), 0);
        assert_eq!(partial_start_len(b"x**\x18B0"), 5);
    }

    #[test]
    fn file_info_parsing() {
        let (n, size, mtime, mode) =
            parse_file_info(b"dir/a b.bin\x001234 14371234567 100755 0 1 1234\x00")
                .unwrap_or_else(|| panic!("parse"));
        assert_eq!((n.as_str(), size, mode), ("dir/a b.bin", 1234, 0o100755));
        assert_eq!(mtime, 0o14371234567);
        assert!(parse_file_info(b"no terminator").is_none());
    }

    async fn roundtrip(files: Vec<(&str, Vec<u8>)>) {
        let src = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut list = Vec::new();
        for (name, data) in &files {
            let p = src.path().join(name);
            std::fs::write(&p, data).unwrap_or_else(|e| panic!("{e}"));
            list.push(SendFile::from_path(&p).unwrap_or_else(|| panic!("name")));
        }
        let (mut a, mut b) = loopback();
        let cancel = CancellationToken::new();
        let dest = dst.path().to_owned();
        let c2 = cancel.clone();
        let rx = tokio::spawn(async move {
            // The "sz" side would start with ZRQINIT; we are rz and speak first.
            let init = hex_header(ZRQINIT, [0; 4]);
            receive(&mut a, &init, &dest, &c2, &mut |_| {}).await
        });
        // The sender starts from the receiver's ZRINIT.
        let first = tokio::time::timeout(Duration::from_secs(5), b.rx.recv())
            .await
            .unwrap_or_else(|_| panic!("no ZRINIT"))
            .unwrap_or_else(|| panic!("closed"));
        let mut seen = Vec::new();
        let sent = send(&mut b, &first, &list, &cancel, &mut |p| seen.push(p.done)).await;
        let got = rx.await.unwrap_or_else(|e| panic!("{e}"));
        let sent = sent.unwrap_or_else(|e| panic!("send: {e}"));
        let got = got.unwrap_or_else(|e| panic!("receive: {e}"));
        assert_eq!(sent.files.len(), files.len());
        assert_eq!(got.files.len(), files.len());
        for (name, data) in &files {
            let back = std::fs::read(dst.path().join(name)).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(crc32(&back), crc32(data), "{name}");
            assert_eq!(back.len(), data.len(), "{name}");
        }
        assert!(files.is_empty() || seen.last().is_some());
    }

    // ---- limits and partial files -------------------------------------------

    #[test]
    fn a_data_subpacket_without_terminator_is_capped_and_errors() {
        let mut d = Decoder::new();
        let mut q = VecDeque::new();
        d.push(&bin_header(ZDATA, [0; 4], true, false), &mut q);
        assert!(matches!(q.pop_front(), Some(Item::Header(_))));
        let mut worst = 0;
        let mut errors = 0;
        // 4 MiB of payload bytes and escaped bytes, never a terminator.
        let chunk: Vec<u8> = (0..1024)
            .map(|i| if i % 3 == 0 { b'a' } else { b'b' })
            .collect();
        for _ in 0..4096 {
            d.push(&chunk, &mut q);
            worst = worst.max(d.buffered());
            while let Some(i) = q.pop_front() {
                assert!(matches!(i, Item::DataError), "{i:?}");
                errors += 1;
            }
        }
        assert!(worst <= MAX_SUBPACKET + 1, "buffered {worst} bytes");
        assert!(errors >= 1);
        // Escaped bytes count after unescaping.
        let mut d = Decoder::new();
        let mut q = VecDeque::new();
        d.push(&bin_header(ZDATA, [0; 4], true, false), &mut q);
        q.clear();
        let esc: Vec<u8> = [ZDLE, b'A'].repeat(100_000);
        d.push(&esc, &mut q);
        assert!(d.buffered() <= MAX_SUBPACKET + 1);
        assert!(q.iter().any(|i| matches!(i, Item::DataError)));
    }

    #[test]
    fn a_full_size_subpacket_still_passes_and_decoding_resumes_after_an_oversized_one() {
        let ok = vec![7u8; MAX_SUBPACKET];
        let items = decode_all(
            &[
                bin_header(ZDATA, [0; 4], true, false),
                data_subpacket(&ok, ZCRCE, true, false),
            ]
            .concat(),
        );
        assert!(matches!(&items[1], Item::Data { data, .. } if data.len() == MAX_SUBPACKET));
        // An oversized run, then a header and a good block: the decoder finds
        // its way back.
        let big = vec![7u8; MAX_SUBPACKET * 3];
        let items = decode_all(
            &[
                bin_header(ZDATA, [0; 4], true, false),
                big,
                bin_header(ZDATA, [0; 4], true, false),
                data_subpacket(b"hello", ZCRCE, true, false),
            ]
            .concat(),
        );
        assert!(items.iter().any(|i| matches!(i, Item::DataError)));
        assert!(
            items
                .iter()
                .any(|i| matches!(i, Item::Data { data, .. } if data == b"hello"))
        );
    }

    /// Reads what the receiver says to us, header by header.
    struct Peer {
        link: Link,
        dec: Decoder,
        q: VecDeque<Item>,
    }

    impl Peer {
        fn new(link: Link) -> Self {
            Self {
                link,
                dec: Decoder::new(),
                q: VecDeque::new(),
            }
        }

        async fn header(&mut self) -> Header {
            loop {
                while let Some(i) = self.q.pop_front() {
                    if let Item::Header(h) = i {
                        return h;
                    }
                }
                let c = tokio::time::timeout(Duration::from_secs(5), self.link.rx.recv())
                    .await
                    .unwrap_or_else(|_| panic!("the receiver said nothing"))
                    .unwrap_or_else(|| panic!("closed"));
                self.dec.push(&c, &mut self.q);
            }
        }

        async fn say(&self, b: Vec<u8>) {
            self.link.send(b).await.unwrap_or_else(|e| panic!("{e}"));
        }

        /// ZFILE for `name` (size announced as `size`).
        async fn offer(&mut self, name: &str, size: usize) {
            let info = format!("{name}\0{size} 0 100644 0 1 {size}\0");
            self.say(bin_header(ZFILE, [0, 0, 0, 1], true, false)).await;
            self.say(data_subpacket(info.as_bytes(), ZCRCW, true, false))
                .await;
            let h = self.header().await;
            assert_eq!(h.kind, ZRPOS, "{h:?}");
            assert_eq!(h.pos(), 0);
        }
    }

    fn files_in(d: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Start `receive` into `dest` and hand back the sender's end.
    async fn start_receiver(
        dest: &std::path::Path,
    ) -> (
        Peer,
        CancellationToken,
        tokio::task::JoinHandle<Result<Outcome, XferError>>,
    ) {
        let (mut a, b) = loopback();
        let cancel = CancellationToken::new();
        let dest = dest.to_owned();
        let c2 = cancel.clone();
        let job = tokio::spawn(async move {
            receive(
                &mut a,
                &hex_header(ZRQINIT, [0; 4]),
                &dest,
                &c2,
                &mut |_| {},
            )
            .await
        });
        let mut peer = Peer::new(b);
        assert_eq!(peer.header().await.kind, ZRINIT);
        (peer, cancel, job)
    }

    #[tokio::test]
    async fn an_endless_zdata_stream_is_answered_with_zrpos_and_never_buffered() {
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (mut peer, cancel, job) = start_receiver(dst.path()).await;
        peer.offer("endless.bin", 1_000_000).await;
        // ZDATA, then plain bytes and no terminator, much more than the cap.
        peer.say(bin_header(ZDATA, [0; 4], true, false)).await;
        for _ in 0..8 {
            peer.say(vec![b'x'; MAX_SUBPACKET]).await;
        }
        let h = peer.header().await;
        assert_eq!((h.kind, h.pos()), (ZRPOS, 0), "{h:?}");
        // Nothing was written, the transfer is still alive and resyncs: a
        // proper block now arrives.
        peer.say(bin_header(ZDATA, [0; 4], true, false)).await;
        peer.say(data_subpacket(b"abc", ZCRCG, true, false)).await;
        cancel.cancel();
        let r = job.await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.err(), Some(XferError::Cancelled));
        assert!(
            files_in(dst.path()).is_empty(),
            "{:?}",
            files_in(dst.path())
        );
    }

    #[tokio::test]
    async fn a_cancelled_download_leaves_no_file_under_the_real_name() {
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (mut peer, cancel, job) = start_receiver(dst.path()).await;
        peer.offer("half.bin", 100_000).await;
        peer.say(bin_header(ZDATA, [0; 4], true, false)).await;
        peer.say(data_subpacket(&sample(4096), ZCRCG, true, false))
            .await;
        // Data lands in the .part file; the real name does not exist.
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if std::fs::metadata(dst.path().join("half.bin.part")).is_ok_and(|m| m.len() == 4096) {
                break;
            }
        }
        assert_eq!(files_in(dst.path()), ["half.bin.part"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let m = std::fs::metadata(dst.path().join("half.bin.part"))
                .map(|m| m.permissions().mode() & 0o777);
            assert_eq!(m.ok(), Some(0o600));
        }
        cancel.cancel();
        let r = job.await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.err(), Some(XferError::Cancelled));
        assert!(
            files_in(dst.path()).is_empty(),
            "{:?}",
            files_in(dst.path())
        );
    }

    #[tokio::test]
    async fn the_other_side_cancelling_removes_the_partial_file() {
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (mut peer, _cancel, job) = start_receiver(dst.path()).await;
        peer.offer("gone.bin", 100_000).await;
        peer.say(bin_header(ZDATA, [0; 4], true, false)).await;
        peer.say(data_subpacket(&sample(2048), ZCRCG, true, false))
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        peer.say(vec![CAN; 5]).await;
        let r = job.await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.err(), Some(XferError::RemoteCancelled));
        assert!(
            files_in(dst.path()).is_empty(),
            "{:?}",
            files_in(dst.path())
        );
    }

    #[tokio::test]
    async fn a_new_zfile_mid_file_drops_the_unfinished_one_and_eof_publishes_the_next() {
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (mut peer, cancel, job) = start_receiver(dst.path()).await;
        peer.offer("first.bin", 100_000).await;
        peer.say(bin_header(ZDATA, [0; 4], true, false)).await;
        peer.say(data_subpacket(&sample(1024), ZCRCE, true, false))
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        // The sender gives up on first.bin and offers another file.
        peer.offer("second.bin", 5).await;
        assert_eq!(files_in(dst.path()), ["second.bin.part"]);
        peer.say(bin_header(ZDATA, [0; 4], true, false)).await;
        peer.say(data_subpacket(b"hello", ZCRCE, true, false)).await;
        peer.say(bin_header(ZEOF, pos_arg(5), true, false)).await;
        assert_eq!(peer.header().await.kind, ZRINIT);
        assert_eq!(files_in(dst.path()), ["second.bin"]);
        assert_eq!(
            std::fs::read(dst.path().join("second.bin")).unwrap_or_default(),
            b"hello"
        );
        cancel.cancel();
        let _ = job.await;
        assert_eq!(files_in(dst.path()), ["second.bin"]);
    }

    #[tokio::test]
    async fn a_dropped_receive_task_removes_the_partial_file() {
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (mut peer, _cancel, job) = start_receiver(dst.path()).await;
        peer.offer("abort.bin", 100_000).await;
        assert_eq!(files_in(dst.path()), ["abort.bin.part"]);
        job.abort();
        let _ = job.await;
        assert!(
            files_in(dst.path()).is_empty(),
            "{:?}",
            files_in(dst.path())
        );
    }

    #[tokio::test]
    async fn send_to_receive_loopback() {
        roundtrip(vec![
            ("one.bin", sample(300_000)),
            ("empty", Vec::new()),
            ("exact", sample(BLOCK * 4)),
            ("tiny", b"x".to_vec()),
        ])
        .await;
    }

    #[tokio::test]
    async fn receiver_never_overwrites() {
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dst.path().join("f.txt"), b"old").unwrap_or_else(|e| panic!("{e}"));
        let src = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let p = src.path().join("f.txt");
        std::fs::write(&p, b"new").unwrap_or_else(|e| panic!("{e}"));
        let (mut a, mut b) = loopback();
        let cancel = CancellationToken::new();
        let dest = dst.path().to_owned();
        let c2 = cancel.clone();
        let rx = tokio::spawn(async move {
            receive(
                &mut a,
                &hex_header(ZRQINIT, [0; 4]),
                &dest,
                &c2,
                &mut |_| {},
            )
            .await
        });
        let first = b.rx.recv().await.unwrap_or_default();
        let list = [SendFile::from_path(&p).unwrap_or_else(|| panic!("name"))];
        send(&mut b, &first, &list, &cancel, &mut |_| {})
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        rx.await
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            std::fs::read(dst.path().join("f.txt")).unwrap_or_default(),
            b"old"
        );
        assert_eq!(
            std::fs::read(dst.path().join("f (1).txt")).unwrap_or_default(),
            b"new"
        );
    }

    #[tokio::test]
    async fn cancel_sends_the_cancel_sequence() {
        let src = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let p = src.path().join("big");
        std::fs::write(&p, sample(2_000_000)).unwrap_or_else(|e| panic!("{e}"));
        let (mut a, mut b) = loopback();
        let cancel = CancellationToken::new();
        // The receiver never answers after ZRINIT: the sender waits for acks
        // until cancelled.
        let init = hex_header(ZRINIT, [0, 0, 0, 0x23]);
        let list = [SendFile::from_path(&p).unwrap_or_else(|| panic!("name"))];
        let c2 = cancel.clone();
        let job = tokio::spawn(async move {
            let r = send(&mut b, &init, &list, &c2, &mut |_| {}).await;
            (r, b)
        });
        // Wait for the ZFILE, answer ZRPOS 0, then let it stream a little.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut wire = Vec::new();
        while let Ok(c) = a.rx.try_recv() {
            wire.extend_from_slice(&c);
        }
        assert!(!wire.is_empty());
        a.send(hex_header(ZRPOS, [0; 4]))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel();
        let (r, _b) = job.await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.err(), Some(XferError::Cancelled));
        let mut tail = Vec::new();
        while let Ok(c) = a.rx.try_recv() {
            tail.extend_from_slice(&c);
        }
        assert!(tail.windows(8).any(|w| w == [CAN; 8]), "no cancel sequence");
    }

    #[tokio::test]
    async fn remote_cancel_is_reported() {
        let dst = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (mut a, b) = loopback();
        let cancel = CancellationToken::new();
        let dest = dst.path().to_owned();
        let job = tokio::spawn(async move {
            receive(
                &mut a,
                &hex_header(ZRQINIT, [0; 4]),
                &dest,
                &cancel,
                &mut |_| {},
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        b.send(vec![CAN; 5]).await.unwrap_or_else(|e| panic!("{e}"));
        let r = job.await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.err(), Some(XferError::RemoteCancelled));
    }

    /// The real thing: lrzsz's `sz` sends a binary file through a pipe and
    /// we receive it; `rz` receives ours. Skipped when lrzsz is not
    /// installed.
    #[tokio::test]
    async fn against_lrzsz() {
        if std::process::Command::new("sz")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("lrzsz not installed; skipping");
            return;
        }
        let work = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let big = sample(1_500_000);
        let tricky: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
        std::fs::write(work.path().join("big.bin"), &big).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(work.path().join("tricky.bin"), &tricky).unwrap_or_else(|e| panic!("{e}"));
        for escape_flag in [&[][..], &["-e"][..]] {
            // Download: sz -> us.
            let out = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
            let mut args: Vec<&str> = vec!["sz", "-b"];
            args.extend_from_slice(escape_flag);
            args.extend(["big.bin", "tricky.bin"]);
            let (mut link, mut child) = spawn_link(&args, work.path()).await;
            let cancel = CancellationToken::new();
            // sz prints "rz\r" and ZRQINIT first; wait for them.
            let mut first = Vec::new();
            while find_start(&first).is_none() {
                let c = tokio::time::timeout(Duration::from_secs(10), link.rx.recv())
                    .await
                    .unwrap_or_else(|_| panic!("sz said nothing"))
                    .unwrap_or_else(|| panic!("sz closed"));
                first.extend_from_slice(&c);
            }
            let (at, kind) = find_start(&first).unwrap_or_else(|| panic!("start"));
            assert_eq!(kind, Start::Download);
            let got = receive(&mut link, &first[at..], out.path(), &cancel, &mut |_| {})
                .await
                .unwrap_or_else(|e| panic!("receive from sz {escape_flag:?}: {e}"));
            assert_eq!(got.files.len(), 2);
            assert_eq!(
                std::fs::read(out.path().join("big.bin")).unwrap_or_default(),
                big
            );
            assert_eq!(
                std::fs::read(out.path().join("tricky.bin")).unwrap_or_default(),
                tricky
            );
            let _ = child.wait().await;
        }
        // Upload: us -> rz.
        let out = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let (mut link, mut child) = spawn_link(&["rz", "-b", "-y"], out.path()).await;
        let cancel = CancellationToken::new();
        let mut first = Vec::new();
        while find_start(&first).is_none() {
            let c = tokio::time::timeout(Duration::from_secs(10), link.rx.recv())
                .await
                .unwrap_or_else(|_| panic!("rz said nothing"))
                .unwrap_or_else(|| panic!("rz closed"));
            first.extend_from_slice(&c);
        }
        let (at, kind) = find_start(&first).unwrap_or_else(|| panic!("start"));
        assert_eq!(kind, Start::Upload);
        let files = [
            SendFile::from_path(&work.path().join("big.bin")).unwrap_or_else(|| panic!("n")),
            SendFile::from_path(&work.path().join("tricky.bin")).unwrap_or_else(|| panic!("n")),
        ];
        let sent = send(&mut link, &first[at..], &files, &cancel, &mut |_| {})
            .await
            .unwrap_or_else(|e| panic!("send to rz: {e}"));
        assert_eq!(sent.files.len(), 2);
        let _ = child.wait().await;
        assert_eq!(
            std::fs::read(out.path().join("big.bin")).unwrap_or_default(),
            big
        );
        assert_eq!(
            std::fs::read(out.path().join("tricky.bin")).unwrap_or_default(),
            tricky
        );
    }
}
