use std::collections::HashMap;
use std::sync::Arc;

use hostwick_core::{Backoff, HostProfile, ProfileId};
use tokio::sync::Mutex;

use crate::{ConnectContext, Connection, SshError};

/// One profile's connection, guarded so concurrent connects are deduplicated.
type Slot = Arc<Mutex<Option<Connection>>>;

/// One live connection per profile, shared by every tab, file pane and tunnel
/// that targets it. Concurrent requests for the same profile wait for a single
/// connect (one host-key prompt, one MFA prompt) instead of racing.
#[derive(Default, Clone)]
pub struct ConnectionPool {
    slots: Arc<Mutex<HashMap<ProfileId, Slot>>>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_or_connect(
        &self,
        profile: &HostProfile,
        ctx: &ConnectContext,
        backoff: Option<Backoff>,
    ) -> Result<Connection, SshError> {
        let slot = {
            let mut slots = self.slots.lock().await;
            slots.entry(profile.id).or_default().clone()
        };
        // Per-profile lock: other profiles connect in parallel.
        let mut guard = slot.lock().await;
        if let Some(c) = guard.as_ref().filter(|c| !c.is_closed()) {
            return Ok(c.clone());
        }
        let conn = match backoff {
            Some(b) => Connection::connect_with_retry(profile, ctx, b).await?,
            None => Connection::connect(profile, ctx).await?,
        };
        *guard = Some(conn.clone());
        Ok(conn)
    }

    /// Existing live connection, without connecting.
    pub async fn get(&self, id: ProfileId) -> Option<Connection> {
        let slot = self.slots.lock().await.get(&id).cloned()?;
        let guard = slot.lock().await;
        guard.as_ref().filter(|c| !c.is_closed()).cloned()
    }

    pub async fn disconnect(&self, id: ProfileId) {
        let slot = self.slots.lock().await.remove(&id);
        if let Some(slot) = slot
            && let Some(c) = slot.lock().await.take()
        {
            c.disconnect().await;
        }
    }

    pub async fn disconnect_all(&self) {
        let slots: Vec<_> = self.slots.lock().await.drain().map(|(_, s)| s).collect();
        for slot in slots {
            if let Some(c) = slot.lock().await.take() {
                c.disconnect().await;
            }
        }
    }
}
