//! A desktop notification for things that finish while the window is not
//! in front (a long transfer). Uses what the system ships with, in a
//! thread of its own so the interface never waits for it: `notify-send`
//! on Linux, `osascript` on macOS. On Windows there is no such helper, so
//! only the in-app notification is shown.

/// Show a notification; failures (no notification daemon) are ignored.
pub fn send(title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    {
        let (title, body) = (title.to_owned(), body.to_owned());
        std::thread::spawn(move || {
            let _ = std::process::Command::new("notify-send")
                .args(["--app-name=Hostwick", "--icon=hostwick", "--"])
                .arg(title)
                .arg(body)
                .status();
        });
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            applescript_escape(body),
            applescript_escape(title)
        );
        std::thread::spawn(move || {
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .status();
        });
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (title, body);
    }
}

/// Text inside an AppleScript string literal.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_escape(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::applescript_escape;

    #[test]
    fn quotes_and_control_characters_cannot_break_out_of_the_script() {
        assert_eq!(applescript_escape(r#"a "b" \ c"#), r#"a \"b\" \\ c"#);
        assert_eq!(applescript_escape("line\nbreak"), "linebreak");
        assert_eq!(applescript_escape("done: 3 files"), "done: 3 files");
    }
}
