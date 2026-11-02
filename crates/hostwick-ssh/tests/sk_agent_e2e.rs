//! A FIDO security key (`sk-ssh-ed25519@openssh.com`) held by the agent
//! logs in to a real OpenSSH server: the agent signs, we pass its
//! security-key signature (flags + counter) through, and tell the person to
//! touch the key.
//!
//! No hardware: a fake agent signs like a security key would (Ed25519 over
//! SHA-256(application) | flags | counter | SHA-256(data)).
//!
//! Skipped unless `HOSTWICK_NET_PORT` and `HOSTWICK_NET_SK_AUTH_KEYS` (an
//! authorized_keys file the server reads, writable by this test) are set;
//! see `scripts/e2e-net.sh`. Its own test binary because it points
//! `SSH_AUTH_SOCK` at the fake agent.
#![allow(clippy::unwrap_used, clippy::expect_used, unsafe_code)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hostwick_core::{AuthMethod, HostProfile, SecretString};
use hostwick_ssh::{
    ConnectContext, Connection, HostKeyDecision, HostKeyInfo, Interaction, NoCredentials,
};
use russh::keys::ssh_key::encoding::Encode;
use russh::keys::ssh_key::sha2::{Digest, Sha256};
use russh::keys::ssh_key::{PublicKey, private, public};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

const SK_ALG: &str = "sk-ssh-ed25519@openssh.com";
const APPLICATION: &str = "ssh:";

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

fn put_string(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&u32::try_from(b.len()).unwrap().to_be_bytes());
    out.extend_from_slice(b);
}

fn get_string<'a>(r: &mut &'a [u8]) -> &'a [u8] {
    let n = u32::from_be_bytes(r[..4].try_into().unwrap()) as usize;
    let (s, rest) = r[4..].split_at(n);
    *r = rest;
    s
}

/// Serve one agent connection: list the security key, sign as it would.
async fn serve_agent(
    mut s: tokio::net::UnixStream,
    blob: Vec<u8>,
    keypair: Arc<private::Ed25519Keypair>,
) {
    let mut counter = 0u32;
    loop {
        let Ok(len) = s.read_u32().await else { return };
        let mut msg = vec![0u8; len as usize];
        if s.read_exact(&mut msg).await.is_err() {
            return;
        }
        let mut reply = Vec::new();
        match msg[0] {
            11 => {
                reply.push(12);
                reply.extend_from_slice(&1u32.to_be_bytes());
                put_string(&mut reply, &blob);
                put_string(&mut reply, b"fake security key");
            }
            13 => {
                let mut r = &msg[1..];
                let key = get_string(&mut r);
                let data = get_string(&mut r);
                assert_eq!(key, blob.as_slice());
                counter += 1;
                let flags = 0x01u8; // user present
                let mut signed = Sha256::digest(APPLICATION.as_bytes()).to_vec();
                signed.push(flags);
                signed.extend_from_slice(&counter.to_be_bytes());
                signed.extend_from_slice(&Sha256::digest(data));
                let sig: russh::keys::ssh_key::Signature =
                    signature::Signer::try_sign(keypair.as_ref(), &signed).unwrap();
                let mut sigblob = Vec::new();
                put_string(&mut sigblob, SK_ALG.as_bytes());
                put_string(&mut sigblob, sig.as_bytes());
                sigblob.push(flags);
                sigblob.extend_from_slice(&counter.to_be_bytes());
                reply.push(14);
                put_string(&mut reply, &sigblob);
            }
            _ => reply.push(5),
        }
        let mut framed = u32::try_from(reply.len()).unwrap().to_be_bytes().to_vec();
        framed.extend_from_slice(&reply);
        if s.write_all(&framed).await.is_err() {
            return;
        }
    }
}

#[test]
fn security_key_in_agent_signs_login() {
    let (Some(port), Some(auth_keys)) = (
        std::env::var("HOSTWICK_NET_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok()),
        std::env::var("HOSTWICK_NET_SK_AUTH_KEYS").ok(),
    ) else {
        eprintln!("skipping: HOSTWICK_NET_PORT / HOSTWICK_NET_SK_AUTH_KEYS not set");
        return;
    };
    let user = std::env::var("HOSTWICK_NET_USER").unwrap_or_else(|_| "hwnet".into());

    let keypair = Arc::new(private::Ed25519Keypair::from_seed(&[42u8; 32]));
    let sk_public = public::KeyData::SkEd25519(public::SkEd25519::new(keypair.public, APPLICATION));
    let blob = sk_public.encode_vec().unwrap();
    let line = PublicKey::new(sk_public, "hostwick-sk-test")
        .to_openssh()
        .unwrap();
    std::fs::write(&auth_keys, format!("{line}\n")).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("agent.sock");
    // SAFETY: this binary has a single test, and nothing else runs yet: no
    // runtime, no other threads reading the environment.
    unsafe { std::env::set_var("SSH_AUTH_SOCK", &sock) };

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let listener = UnixListener::bind(&sock).unwrap();
        let kp = keypair.clone();
        let b = blob.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                tokio::spawn(serve_agent(s, b.clone(), kp.clone()));
            }
        });

        let ix = Arc::new(Notices::default());
        let mut p = HostProfile::new("sk", "127.0.0.1");
        p.port = port;
        p.user = Some(user.clone());
        p.auth = vec![AuthMethod::Agent];
        let mut ctx = ConnectContext::new(ix.clone(), Arc::new(NoCredentials));
        ctx.known_hosts = None;
        ctx.connect_timeout = Duration::from_secs(15);
        let conn = Connection::connect(&p, &ctx).await.unwrap();
        let out = conn.exec("whoami").await.unwrap();
        assert_eq!(out.stdout_lossy().trim(), user);
        let notices = ix.0.lock().unwrap().clone();
        assert!(
            notices
                .iter()
                .any(|n| n.contains("touch your security key")),
            "{notices:?}"
        );
    });
    std::fs::write(&auth_keys, "").ok();
}
