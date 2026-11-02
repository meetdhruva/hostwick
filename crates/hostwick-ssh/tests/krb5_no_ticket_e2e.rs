//! Kerberos configured but no ticket: the method is skipped with "run
//! kinit" and the next method logs in. Its own test binary because it points
//! `KRB5CCNAME` at an empty cache. Skipped unless `HOSTWICK_NET_KRB5` is set
//! (see `scripts/e2e-net.sh`).
#![allow(clippy::unwrap_used, clippy::expect_used, unsafe_code)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hostwick_core::{AuthMethod, HostProfile, SecretString};
use hostwick_ssh::{
    ConnectContext, Connection, HostKeyDecision, HostKeyInfo, Interaction, NoCredentials,
};

#[derive(Default)]
struct Notices(Mutex<Vec<String>>);

#[async_trait]
impl Interaction for Notices {
    async fn host_key(&self, _: &HostKeyInfo) -> HostKeyDecision {
        HostKeyDecision::AcceptOnce
    }
    async fn password(&self, _: &str, _: &str) -> Option<SecretString> {
        None
    }
    async fn key_passphrase(&self, _: &Path) -> Option<SecretString> {
        None
    }
    async fn keyboard_interactive(
        &self,
        _: &str,
        _: &str,
        _: &[(String, bool)],
    ) -> Option<Vec<String>> {
        None
    }
    fn notice(&self, text: &str) {
        eprintln!("notice: {text}");
        self.0.lock().unwrap().push(text.to_owned());
    }
}

#[test]
fn missing_ticket_falls_through_to_next_method() {
    let (Some(port), Some(_), Some(dir)) = (
        std::env::var("HOSTWICK_NET_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok()),
        std::env::var("HOSTWICK_NET_KRB5").ok(),
        std::env::var("HOSTWICK_NET_DIR").ok(),
    ) else {
        eprintln!("skipping: HOSTWICK_NET_PORT / HOSTWICK_NET_KRB5 not set");
        return;
    };
    let empty = tempfile::tempdir().unwrap();
    // SAFETY: single test in this binary, set before any runtime or other
    // thread exists.
    unsafe {
        std::env::set_var(
            "KRB5CCNAME",
            format!("FILE:{}", empty.path().join("none").display()),
        )
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let ix = Arc::new(Notices::default());
        let mut p = HostProfile::new("krb", "localhost");
        p.port = port;
        p.user = Some(std::env::var("HOSTWICK_NET_USER").unwrap_or_else(|_| "hwnet".into()));
        p.auth = vec![
            AuthMethod::Gssapi { delegate: true },
            AuthMethod::KeyFile {
                path: Path::new(&dir).join("client_key"),
                passphrase_credential: None,
            },
        ];
        let mut ctx = ConnectContext::new(ix.clone(), Arc::new(NoCredentials));
        ctx.known_hosts = None;
        ctx.connect_timeout = Duration::from_secs(15);
        let conn = Connection::connect(&p, &ctx).await.unwrap();
        assert!(conn.exec("true").await.unwrap().exit_status == Some(0));
        let notices = ix.0.lock().unwrap().clone();
        assert_eq!(
            notices,
            vec!["kerberos: no Kerberos ticket — run kinit".to_owned()]
        );
    });
}
