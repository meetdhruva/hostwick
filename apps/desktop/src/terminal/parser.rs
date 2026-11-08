//! The output side of a terminal session: everything the remote program
//! prints goes through here, off the UI thread, on its way to the screen
//! grid. Besides feeding the grid it
//!
//! - reads OSC 7 (the shell's folder) and OSC 133 (prompt and command
//!   marks) out of the stream,
//! - feeds the text watcher and the session log,
//! - spots the start of a ZMODEM or trzsz transfer and, while one runs,
//!   forwards all output to it instead of the screen.

use alacritty_terminal::Term;
use alacritty_terminal::vte::ansi::Processor;
use bytes::Bytes;
use hostwick_term::ShellCommand;
use hostwick_term::xfer::{Detector, Scan};
use tokio::sync::mpsc;

use super::transfer::{Done, XferStart};
use super::{Listener, Shared, osc7, osc133};

/// Chunks the transfer may fall behind by (each at most one SSH packet).
const FORWARD_CHUNKS: usize = 64;

pub(super) struct Parser {
    processor: Processor,
    osc7: osc7::Osc7Scanner,
    osc133: osc133::Osc133Scanner,
    detector: Detector,
    /// While a transfer runs: where its output goes.
    forwarding: Option<mpsc::Sender<Bytes>>,
    /// Output the transfer had no room for yet (see [`Parser::wait_room`]).
    blocked: Option<Bytes>,
    done: mpsc::UnboundedSender<Done>,
    input: mpsc::Sender<ShellCommand>,
    /// Results since the caller last took them.
    pub cwd: Option<String>,
    pub marks: Vec<osc133::Mark>,
    pub started: Option<XferStart>,
}

impl Parser {
    pub fn new(done: mpsc::UnboundedSender<Done>, input: mpsc::Sender<ShellCommand>) -> Self {
        Self {
            processor: Processor::new(),
            osc7: osc7::Osc7Scanner::default(),
            osc133: osc133::Osc133Scanner::default(),
            detector: Detector::default(),
            forwarding: None,
            blocked: None,
            done,
            input,
            cwd: None,
            marks: Vec::new(),
            started: None,
        }
    }

    /// Output from the remote program.
    pub fn output(&mut self, shared: &Shared, term: &mut Term<Listener>, b: Bytes) {
        let b = match self.forward(b) {
            None => return,
            Some(b) => b,
        };
        match self.detector.scan(&b) {
            Scan::Pass => self.display(shared, term, &b),
            Scan::Split { pass, start } => {
                if !pass.is_empty() {
                    self.display(shared, term, &pass);
                }
                if let Some((detected, initial)) = start {
                    let (tx, rx) = mpsc::channel(FORWARD_CHUNKS);
                    self.forwarding = Some(tx);
                    self.started = Some(XferStart {
                        detected,
                        initial,
                        rx,
                        done: self.done.clone(),
                        input: self.input.clone(),
                    });
                }
            }
        }
    }

    /// Hand `b` to the running transfer. Returns it when nothing is
    /// running (or the transfer is gone), for normal display. When the
    /// transfer is behind, `b` is parked in `blocked` and the caller must
    /// stop reading until [`Parser::wait_room`] returns: that is the back
    /// pressure that makes the remote program slow down to the speed of the
    /// transfer instead of piling up the file in memory.
    fn forward(&mut self, b: Bytes) -> Option<Bytes> {
        let Some(f) = &self.forwarding else {
            return Some(b);
        };
        match f.try_send(b) {
            Ok(()) => None,
            Err(mpsc::error::TrySendError::Full(b)) => {
                self.blocked = Some(b);
                None
            }
            // The transfer is gone without handing the stream back.
            Err(mpsc::error::TrySendError::Closed(b)) => {
                self.forwarding = None;
                Some(b)
            }
        }
    }

    /// Output is parked: the reader must wait before taking more.
    pub fn is_blocked(&self) -> bool {
        self.blocked.is_some()
    }

    /// Wait until the transfer has room for the parked output (and pass
    /// it on), or the transfer ends. Returns the end of the transfer when
    /// that happened first; the parked output is then shown or forwarded
    /// by the caller through [`Parser::unpark`].
    pub async fn wait_room(&mut self, done: &mut mpsc::UnboundedReceiver<Done>) -> Option<Done> {
        while self.blocked.is_some() {
            let f = self.forwarding.clone()?;
            tokio::select! {
                permit = f.reserve() => match permit {
                    Ok(p) => {
                        if let Some(b) = self.blocked.take() {
                            p.send(b);
                        }
                    }
                    Err(_) => {
                        self.forwarding = None;
                        return None;
                    }
                },
                d = done.recv() => return d,
            }
        }
        None
    }

    /// Output that was parked when the transfer ended: normal output now.
    pub fn unpark(&mut self, shared: &Shared, term: &mut Term<Listener>) {
        if let Some(b) = self.blocked.take() {
            self.output(shared, term, b);
        }
    }

    fn display(&mut self, shared: &Shared, term: &mut Term<Listener>, b: &[u8]) {
        if let Some(dir) = self.osc7.feed(b) {
            self.cwd = Some(dir);
        }
        let marks = self.osc133.feed(b);
        if !marks.is_empty() {
            self.marks.extend(marks);
        }
        self.processor.advance(term, b);
        shared.watch.push(b);
        if let Ok(mut log) = shared.log.lock()
            && let Some(l) = log.as_mut()
            && let Err(e) = l.write(b)
        {
            tracing::warn!(error = %e, "session log write failed; logging stopped");
            *log = None;
        }
    }

    /// Bytes that might have started a transfer sequence were held back;
    /// nothing followed, so they were ordinary output.
    pub fn is_holding(&self) -> bool {
        self.detector.is_holding()
    }

    pub fn flush_held(&mut self, shared: &Shared, term: &mut Term<Listener>) {
        let held = self.detector.flush();
        if !held.is_empty() {
            self.display(shared, term, &held);
        }
    }

    /// The transfer ended: take the stream back, showing what it did not
    /// use.
    pub fn take_back(&mut self, shared: &Shared, term: &mut Term<Listener>, mut done: Done) {
        self.forwarding = None;
        let mut rest = std::mem::take(&mut done.spare);
        while let Ok(b) = done.rx.try_recv() {
            rest.extend_from_slice(&b);
        }
        if !rest.is_empty() {
            self.output(shared, term, Bytes::from(rest));
        }
    }
}
