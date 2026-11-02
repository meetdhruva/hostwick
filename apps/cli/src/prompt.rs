use std::path::Path;

use async_trait::async_trait;
use hostwick_core::SecretString;
use hostwick_ssh::{HostKeyDecision, HostKeyInfo, HostKeyStatus, Interaction};

use crate::tty;

/// `--batch` / `--accept-new`, mirroring `ssh -o BatchMode` and
/// `StrictHostKeyChecking=accept-new`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Policy {
    pub batch: bool,
    pub accept_new: bool,
}

pub struct TerminalInteraction {
    policy: Policy,
}

impl TerminalInteraction {
    pub fn new(policy: Policy) -> Self {
        Self { policy }
    }
}

/// Prompts block on the terminal; run them off the async workers.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

#[async_trait]
impl Interaction for TerminalInteraction {
    async fn host_key(&self, info: &HostKeyInfo) -> HostKeyDecision {
        let target = if info.port == 22 {
            info.host.clone()
        } else {
            format!("[{}]:{}", info.host, info.port)
        };
        match &info.status {
            HostKeyStatus::Changed { line } => {
                eprintln!(
                    "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
                     @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
                     @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
                     Someone could be eavesdropping on you right now (man-in-the-middle),\n\
                     or the host key was legitimately replaced.\n\
                     {target} now presents {} key {}\n\
                     The recorded key is on line {line} of your known_hosts file.",
                    info.algorithm, info.fingerprint
                );
                if self.policy.batch {
                    return HostKeyDecision::Reject;
                }
                let answer = blocking(|| {
                    tty::read_line("Type 'replace' to trust the NEW key, anything else aborts: ")
                })
                .await
                .and_then(Result::ok)
                .unwrap_or_default();
                if answer == "replace" {
                    HostKeyDecision::AcceptAndRemember
                } else {
                    HostKeyDecision::Reject
                }
            }
            _ => {
                if self.policy.accept_new {
                    eprintln!(
                        "Permanently added {target} ({}) {}",
                        info.algorithm, info.fingerprint
                    );
                    return HostKeyDecision::AcceptAndRemember;
                }
                if self.policy.batch {
                    eprintln!(
                        "Host key for {target} is not known and --batch is set ({}).",
                        info.fingerprint
                    );
                    return HostKeyDecision::Reject;
                }
                eprintln!("The authenticity of host '{target}' can't be established.");
                eprintln!(
                    "{} key fingerprint is {}.",
                    info.algorithm, info.fingerprint
                );
                let answer =
                    blocking(|| tty::read_line("Trust it? (yes = remember / once / no): "))
                        .await
                        .and_then(Result::ok)
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                match answer.as_str() {
                    "yes" | "y" => HostKeyDecision::AcceptAndRemember,
                    "once" | "o" => HostKeyDecision::AcceptOnce,
                    _ => HostKeyDecision::Reject,
                }
            }
        }
    }

    async fn password(&self, user: &str, host: &str) -> Option<SecretString> {
        if self.policy.batch {
            return None;
        }
        let prompt = format!("{user}@{host}'s password: ");
        blocking(move || tty::read_secret(&prompt)).await?.ok()
    }

    async fn key_passphrase(&self, path: &Path) -> Option<SecretString> {
        if self.policy.batch {
            return None;
        }
        let prompt = format!("Enter passphrase for key '{}': ", path.display());
        blocking(move || tty::read_secret(&prompt)).await?.ok()
    }

    async fn keyboard_interactive(
        &self,
        name: &str,
        instructions: &str,
        prompts: &[(String, bool)],
    ) -> Option<Vec<String>> {
        if self.policy.batch {
            return None;
        }
        for line in [name, instructions] {
            if !line.is_empty() {
                eprintln!("{line}");
            }
        }
        let prompts = prompts.to_vec();
        blocking(move || {
            prompts
                .iter()
                .map(|(p, echo)| {
                    if *echo {
                        tty::read_line(p)
                    } else {
                        tty::read_secret(p)
                            .map(|s| hostwick_core::ExposeSecret::expose_secret(&s).to_owned())
                    }
                })
                .collect::<anyhow::Result<Vec<_>>>()
                .ok()
        })
        .await?
    }

    fn banner(&self, text: &str) {
        eprint!("{text}");
    }
}
