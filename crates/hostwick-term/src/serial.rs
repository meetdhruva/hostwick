//! Serial consoles (RS-232, USB adapters) via the `serialport` crate.
//!
//! The port is blocking, so it gets two plain threads: one reads (with a
//! short timeout so it notices when the session is closed) and one writes,
//! sends BREAK and closes. Neither ever runs on the async runtime.

use std::io::{ErrorKind as IoKind, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::Bytes;
use hostwick_core::{FlowControl, Parity, SerialSettings};
use serialport::{DataBits, SerialPort, StopBits};
use tokio::sync::mpsc;

use crate::{
    Control, QUEUE, ShellCommand, ShellEvent, TermError, TerminalSession, TransportKind,
    failure_text,
};

/// A serial port found on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// What to open: `COM3`, `/dev/ttyUSB0`, `/dev/cu.usbserial-1410`.
    pub name: String,
    /// Human-readable, e.g. `USB serial (FTDI FT232R USB UART)`; may be empty.
    pub description: String,
}

/// Serial ports present now, sorted by name. Never fails: an enumeration
/// problem just yields an empty list (the user can still type a name).
pub fn list_ports() -> Vec<PortInfo> {
    // serialport's sysfs scan panics when /sys/class/tty is missing
    // (some containers); check first.
    if cfg!(target_os = "linux") && !std::path::Path::new("/sys/class/tty").is_dir() {
        return Vec::new();
    }
    let mut ports: Vec<PortInfo> = match serialport::available_ports() {
        Ok(list) => list
            .into_iter()
            .map(|p| PortInfo {
                description: describe(&p.port_type),
                name: p.port_name,
            })
            .collect(),
        Err(e) => {
            tracing::debug!(error = %e, "cannot list serial ports");
            Vec::new()
        }
    };
    // macOS lists every device twice (`tty.*` and `cu.*`); `cu.*` is the
    // one to open for an outgoing connection.
    if cfg!(target_os = "macos") {
        ports.retain(|p| !p.name.starts_with("/dev/tty."));
    }
    ports.sort_by_key(|a| natural_key(&a.name));
    ports.dedup_by(|a, b| a.name == b.name);
    ports
}

/// Sort `COM10` after `COM9`.
fn natural_key(name: &str) -> (String, u64) {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (stem, num) = name.split_at(name.len() - digits);
    (stem.to_owned(), num.parse().unwrap_or(0))
}

fn describe(t: &serialport::SerialPortType) -> String {
    use serialport::SerialPortType as T;
    match t {
        T::UsbPort(u) => {
            let what = [u.manufacturer.as_deref(), u.product.as_deref()]
                .into_iter()
                .flatten()
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if what.is_empty() {
                format!("USB serial ({:04x}:{:04x})", u.vid, u.pid)
            } else {
                format!("USB serial ({what})")
            }
        }
        T::BluetoothPort => "Bluetooth serial".into(),
        T::PciPort => "Built-in serial port".into(),
        T::Unknown => String::new(),
    }
}

/// Check settings before touching the device, with a clear message.
pub fn validate(settings: &SerialSettings) -> Result<(), TermError> {
    if settings.baud == 0 {
        return Err(TermError::InvalidSettings(
            "baud rate must be above 0".into(),
        ));
    }
    if !(5..=8).contains(&settings.data_bits) {
        return Err(TermError::InvalidSettings(format!(
            "data bits must be 5, 6, 7 or 8 (got {})",
            settings.data_bits
        )));
    }
    if !matches!(settings.stop_bits, 1 | 2) {
        return Err(TermError::InvalidSettings(format!(
            "stop bits must be 1 or 2 (got {})",
            settings.stop_bits
        )));
    }
    Ok(())
}

/// `9600 8N1`, `115200 7E2 RTS/CTS`.
pub fn summary(settings: &SerialSettings) -> String {
    let parity = match settings.parity {
        Parity::None => 'N',
        Parity::Odd => 'O',
        Parity::Even => 'E',
    };
    let flow = match settings.flow {
        FlowControl::None => "",
        FlowControl::Software => " XON/XOFF",
        FlowControl::Hardware => " RTS/CTS",
    };
    format!(
        "{} {}{}{}{}",
        settings.baud, settings.data_bits, parity, settings.stop_bits, flow
    )
}

/// Open `port` (blocking work runs on a blocking thread) and start the
/// session.
pub async fn open(port: &str, settings: &SerialSettings) -> Result<TerminalSession, TermError> {
    let port = port.trim().to_owned();
    let settings = settings.clone();
    tokio::task::spawn_blocking(move || open_blocking(&port, &settings))
        .await
        .map_err(|e| TermError::Serial {
            port: String::new(),
            reason: format!("internal error: {e}"),
        })?
}

/// How long one read waits before checking whether the session was closed.
const READ_POLL: Duration = Duration::from_millis(100);
/// How long BREAK holds the line (typical terminal programs use 250–500 ms).
const BREAK_LEN: Duration = Duration::from_millis(300);

/// Blocking open + start; see [`open`].
pub fn open_blocking(port: &str, settings: &SerialSettings) -> Result<TerminalSession, TermError> {
    if port.is_empty() {
        return Err(TermError::InvalidSettings(
            "no serial port selected (e.g. COM3 or /dev/ttyUSB0)".into(),
        ));
    }
    validate(settings)?;
    let data_bits = match settings.data_bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        _ => DataBits::Eight,
    };
    let device = device_path(port);
    let mut sp = serialport::new(&device, settings.baud)
        .data_bits(data_bits)
        .parity(match settings.parity {
            Parity::None => serialport::Parity::None,
            Parity::Odd => serialport::Parity::Odd,
            Parity::Even => serialport::Parity::Even,
        })
        .stop_bits(if settings.stop_bits == 2 {
            StopBits::Two
        } else {
            StopBits::One
        })
        .flow_control(match settings.flow {
            FlowControl::None => serialport::FlowControl::None,
            FlowControl::Software => serialport::FlowControl::Software,
            FlowControl::Hardware => serialport::FlowControl::Hardware,
        })
        .timeout(READ_POLL)
        .open()
        .map_err(|e| open_error(port, &e))?;
    // Many USB adapters and devices only talk once DTR/RTS are raised
    // (PuTTY does the same). Not fatal: some drivers refuse.
    let _ = sp.write_data_terminal_ready(true);
    if settings.flow != FlowControl::Hardware {
        let _ = sp.write_request_to_send(true);
    }
    let reader = sp.try_clone().map_err(|e| TermError::Serial {
        port: port.to_owned(),
        reason: e.to_string(),
    })?;
    let breaker = sp.try_clone().ok();
    Ok(start(port.to_owned(), reader, sp, breaker))
}

/// On Windows `COM10` and above must be opened as `\\.\COM10`; the prefix
/// works for every COM port.
fn device_path(port: &str) -> String {
    if cfg!(windows) && !port.starts_with(r"\\") && port.to_ascii_uppercase().starts_with("COM") {
        format!(r"\\.\{port}")
    } else {
        port.to_owned()
    }
}

fn start(
    name: String,
    mut reader: Box<dyn SerialPort>,
    mut writer: Box<dyn SerialPort>,
    breaker: Option<Box<dyn SerialPort>>,
) -> TerminalSession {
    let breakable = breaker.is_some();
    let (in_tx, mut in_rx) = mpsc::channel::<ShellCommand>(QUEUE);
    let (out_tx, out_rx) = mpsc::channel::<ShellEvent>(QUEUE);
    let (ctl_tx, mut ctl_rx) = mpsc::channel::<Control>(8);
    let stop = Arc::new(AtomicBool::new(false));

    // Reader thread: device → output queue (blocking_send = backpressure).
    {
        let (stop, name, out_tx) = (stop.clone(), name.clone(), out_tx.clone());
        let spawned = std::thread::Builder::new()
            .name(format!("serial-read {name}"))
            .spawn(move || {
                let mut buf = vec![0u8; 8192];
                let failure = loop {
                    if stop.load(Ordering::Acquire) {
                        break None;
                    }
                    match reader.read(&mut buf) {
                        Ok(0) => continue,
                        Ok(n) => {
                            if out_tx
                                .blocking_send(ShellEvent::Stdout(Bytes::copy_from_slice(
                                    &buf[..n],
                                )))
                                .is_err()
                            {
                                break None;
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                IoKind::TimedOut | IoKind::Interrupted | IoKind::WouldBlock
                            ) => {}
                        Err(e) => break Some(format!("serial port {name} disconnected: {e}")),
                    }
                };
                stop.store(true, Ordering::Release);
                if let Some(msg) = failure {
                    tracing::info!("{msg}");
                    let _ = out_tx.blocking_send(ShellEvent::Stderr(failure_text(&msg)));
                }
                let _ = out_tx.blocking_send(ShellEvent::Closed);
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "cannot start serial reader thread");
        }
    }

    // Writer thread: input queue → device. Exits when the input queue
    // closes (the session was dropped), on Close, or when writing fails.
    {
        let (wstop, name) = (stop.clone(), name.clone());
        let spawned = std::thread::Builder::new()
            .name(format!("serial-write {name}"))
            .spawn(move || {
                while let Some(cmd) = in_rx.blocking_recv() {
                    match cmd {
                        ShellCommand::Data(b) => {
                            if let Err(e) = writer.write_all(&b).and_then(|_| writer.flush()) {
                                if e.kind() == IoKind::TimedOut {
                                    // Flow control held us up past the
                                    // timeout; keep going.
                                    tracing::warn!(port = %name, "serial write timed out (flow control?)");
                                } else {
                                    tracing::info!(port = %name, error = %e, "serial write failed");
                                    break;
                                }
                            }
                        }
                        ShellCommand::Close => break,
                        ShellCommand::Resize { .. } | ShellCommand::Eof => {}
                    }
                    if wstop.load(Ordering::Acquire) {
                        break;
                    }
                }
                wstop.store(true, Ordering::Release);
            });
        if let Err(e) = spawned {
            stop.store(true, Ordering::Release);
            tracing::warn!(error = %e, "cannot start serial writer thread");
        }
    }

    // BREAK runs on its own handle and thread so a 300 ms break never
    // delays typing, and typing never delays a break.
    if let Some(breaker) = breaker {
        let spawned = std::thread::Builder::new()
            .name(format!("serial-break {name}"))
            .spawn(move || {
                while let Some(Control::Break) = ctl_rx.blocking_recv() {
                    send_break(&*breaker, &name);
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "cannot start serial break thread");
        }
    }

    let session = TerminalSession::new(in_tx, out_rx, TransportKind::Serial);
    if breakable {
        session.with_control(ctl_tx)
    } else {
        session
    }
}

fn send_break(port: &dyn SerialPort, name: &str) {
    match port.set_break() {
        Ok(()) => {
            std::thread::sleep(BREAK_LEN);
            if let Err(e) = port.clear_break() {
                tracing::warn!(port = %name, error = %e, "cannot clear BREAK");
            }
        }
        Err(e) => tracing::warn!(port = %name, error = %e, "cannot send BREAK"),
    }
}

/// Which OS family's advice to give in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Os {
    Linux,
    Mac,
    Windows,
    Other,
}

const OS: Os = if cfg!(target_os = "linux") {
    Os::Linux
} else if cfg!(target_os = "macos") {
    Os::Mac
} else if cfg!(windows) {
    Os::Windows
} else {
    Os::Other
};

fn open_error(port: &str, e: &serialport::Error) -> TermError {
    let available = || {
        let names: Vec<String> = list_ports().into_iter().map(|p| p.name).collect();
        if names.is_empty() {
            " (no serial ports found — is the adapter plugged in and its driver installed?)"
                .to_owned()
        } else {
            format!(" (available: {})", names.join(", "))
        }
    };
    classify(port, &e.kind, &e.description, OS, available)
}

fn classify(
    port: &str,
    kind: &serialport::ErrorKind,
    description: &str,
    os: Os,
    available: impl FnOnce() -> String,
) -> TermError {
    use serialport::ErrorKind as K;
    let d = description.to_ascii_lowercase();
    let port_s = port.to_owned();
    let busy = d.contains("busy")
        || d.contains("resource temporarily unavailable")
        || d.contains("lock")
        || (os == Os::Windows && d.contains("access is denied"));
    let missing = matches!(kind, K::Io(IoKind::NotFound))
        || d.contains("no such file")
        || d.contains("cannot find")
        || d.contains("not found")
        || (matches!(kind, K::NoDevice) && !busy);
    if busy {
        return TermError::SerialBusy(port_s);
    }
    if missing {
        return TermError::SerialNotFound {
            port: port_s,
            available: available(),
        };
    }
    if matches!(kind, K::Io(IoKind::PermissionDenied)) || d.contains("permission denied") {
        let hint = match os {
            Os::Linux => " — add yourself to the `dialout` group (`sudo usermod -aG dialout $USER`; on Arch/Fedora the group may be `uucp`), then log out and back in".to_owned(),
            Os::Mac => " — check the device's permissions (`ls -l`) and that no other program has it open".to_owned(),
            _ => String::new(),
        };
        return TermError::SerialPermission { port: port_s, hint };
    }
    if matches!(kind, K::InvalidInput) {
        return TermError::InvalidSettings(format!("{port}: {description}"));
    }
    TermError::Serial {
        port: port_s,
        reason: description.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::ErrorKind as K;

    fn none() -> String {
        " (available: COM1)".into()
    }

    #[test]
    fn open_errors_are_explained() {
        let e = classify(
            "/dev/ttyUSB0",
            &K::Io(IoKind::PermissionDenied),
            "Permission denied",
            Os::Linux,
            none,
        );
        assert!(e.to_string().contains("dialout"), "{e}");
        let e = classify(
            "/dev/ttyUSB0",
            &K::NoDevice,
            "Device or resource busy",
            Os::Linux,
            none,
        );
        assert!(matches!(e, TermError::SerialBusy(_)), "{e}");
        assert!(e.to_string().contains("in use"));
        let e = classify(
            "/dev/ttyUSB9",
            &K::Io(IoKind::NotFound),
            "No such file or directory",
            Os::Linux,
            none,
        );
        assert_eq!(
            e.to_string(),
            "serial port /dev/ttyUSB9 not found (available: COM1)"
        );
        let e = classify("COM3", &K::NoDevice, "Access is denied.", Os::Windows, none);
        assert!(matches!(e, TermError::SerialBusy(_)));
        let e = classify(
            "COM9",
            &K::NoDevice,
            "The system cannot find the file specified.",
            Os::Windows,
            none,
        );
        assert!(matches!(e, TermError::SerialNotFound { .. }));
        let e = classify("COM1", &K::InvalidInput, "bad baud", Os::Windows, none);
        assert!(matches!(e, TermError::InvalidSettings(_)));
    }

    #[test]
    fn settings_are_checked_and_summarised() {
        let mut s = SerialSettings::default();
        assert_eq!(summary(&s), "9600 8N1");
        assert!(validate(&s).is_ok());
        s.data_bits = 9;
        assert!(validate(&s).is_err());
        s.data_bits = 7;
        s.parity = Parity::Even;
        s.stop_bits = 2;
        s.flow = FlowControl::Hardware;
        s.baud = 115_200;
        assert_eq!(summary(&s), "115200 7E2 RTS/CTS");
        s.stop_bits = 3;
        assert!(validate(&s).is_err());
        assert!(open_blocking("", &SerialSettings::default()).is_err());
    }

    #[test]
    fn ports_sort_naturally() {
        let mut v = ["COM10", "COM2", "COM1", "/dev/ttyUSB1"];
        v.sort_by_key(|n| natural_key(n));
        assert_eq!(v, ["/dev/ttyUSB1", "COM1", "COM2", "COM10"]);
        // Listing never panics.
        let _ = list_ports();
    }

    #[test]
    fn com_ports_get_the_device_prefix_on_windows() {
        let p = device_path("COM12");
        if cfg!(windows) {
            assert_eq!(p, r"\\.\COM12");
        } else {
            assert_eq!(p, "COM12");
        }
        assert_eq!(device_path("/dev/ttyS0"), "/dev/ttyS0");
    }
}
