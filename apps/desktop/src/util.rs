use gpui_kit::SharedString;
use hostwick_core::Protocol;

/// `1.5 MiB` (`1,5 MiB` where the language writes a decimal comma; French
/// says `Mio`).
pub fn human_size(n: u64) -> String {
    let units = [
        t!("size-b"),
        t!("size-kib"),
        t!("size-mib"),
        t!("size-gib"),
        t!("size-tib"),
    ];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} {}", units[0])
    } else {
        let num = format!("{v:.1}");
        format!("{} {}", localize_decimal(&num), units[u])
    }
}

/// `1.5` → `1,5` for languages that use a decimal comma.
pub fn localize_decimal(num: &str) -> String {
    if matches!(crate::i18n::current(), "de" | "es" | "fr" | "pt-BR") {
        num.replace('.', ",")
    } else {
        num.to_owned()
    }
}

/// The protocol's name in the UI language ("SSH", "Telnet", "Serial").
pub fn protocol_name(p: &Protocol) -> SharedString {
    match p {
        Protocol::Ssh => t!("protocol-ssh"),
        Protocol::Telnet => t!("protocol-telnet"),
        Protocol::Serial(_) => t!("protocol-serial"),
    }
}

/// A shortcut as written on this platform: `ctrl` on Windows/Linux, `mac`
/// on macOS.
pub fn keys(ctrl: &'static str, mac: &'static str) -> &'static str {
    if cfg!(target_os = "macos") { mac } else { ctrl }
}

/// `rwxr-xr-x` from Unix permission bits (lower 9 bits).
pub fn mode_string(mode: u32) -> String {
    let mut out = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        out.push(if bits & 4 != 0 { 'r' } else { '-' });
        out.push(if bits & 2 != 0 { 'w' } else { '-' });
        out.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    out
}

/// `YYYY-MM-DD HH:MM` in the computer's local time zone.
pub fn local_time(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| human_time(secs))
}

/// `YYYY-MM-DD HH:MM` (UTC) from Unix seconds, without a date library.
pub fn human_time(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// The colour of a host environment (tab stripe, ribbon).
pub fn environment_color(e: &hostwick_core::Environment) -> Option<gpui_kit::Hsla> {
    let (r, g, b) = e.rgb()?;
    Some(gpui_kit::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into())
}

/// Spoken name of an environment (none for "no environment" and custom
/// colours, which have no name).
pub fn environment_name(e: &hostwick_core::Environment) -> Option<SharedString> {
    use hostwick_core::Environment;
    match e {
        Environment::Dev => Some(t!("tx-env-dev")),
        Environment::Staging => Some(t!("tx-env-staging")),
        Environment::Prod => Some(t!("tx-env-prod")),
        Environment::None | Environment::Custom(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(human_time(0), "1970-01-01 00:00");
        assert_eq!(mode_string(0o755), "rwxr-xr-x");
        assert_eq!(mode_string(0o100600), "rw-------");
        assert_eq!(human_time(1_600_000_000), "2020-09-13 12:26");
        assert_eq!(human_time(951_782_400), "2000-02-29 00:00");
        assert_eq!(human_size(10), "10 B");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MiB");
    }
}
