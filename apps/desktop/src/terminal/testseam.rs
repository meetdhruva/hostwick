//! Hooks for automated GUI tests. They do nothing unless the matching
//! environment variable is set, which only test scripts do.
//!
//! The operating system's drag and drop cannot be driven from `xdotool`,
//! so `HOSTWICK_TEST_DROP=/a:/b` makes F12 in a terminal behave exactly as
//! if those files had been dropped on it (the same handler runs).

use std::path::PathBuf;

use gpui_kit::Modifiers;

pub fn drop_files(key: &str, m: &Modifiers) -> Option<Vec<PathBuf>> {
    if key != "f12" || m.control || m.alt || m.platform {
        return None;
    }
    let v = std::env::var_os("HOSTWICK_TEST_DROP")?;
    let list: Vec<PathBuf> = std::env::split_paths(&v).collect();
    (!list.is_empty()).then_some(list)
}
