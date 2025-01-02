use std::{cmp::max, str::from_utf8};

use etcd_client::{LockClient, LockOptions};
use tokio::sync::{Mutex, MutexGuard};
use tracing::info;

use crate::{
    error::Result,
    net::{PlaceId, Revision},
};

use super::{FencingToken, LeaseId};
pub struct PlaceLock {
    pub(super) value: Mutex<PlaceLockData>,
    pub(super) prefix: String,
    pub(super) place_id: PlaceId,
    pub(super) lease: LeaseId,
}

pub struct PlaceLockData {
    fencing_token: FencingToken,
    min_revision: Revision,
    lock_client: LockClient,
}

impl PlaceLock {
    pub fn new(lock_client: LockClient, prefix: String, place_id: PlaceId, lease: LeaseId) -> Self {
        let value = Mutex::new(PlaceLockData {
            fencing_token: Default::default(),
            min_revision: Default::default(),
            lock_client,
        });
        PlaceLock { value, prefix, place_id, lease }
    }

    pub fn place_id(&self) -> PlaceId {
        self.place_id
    }

    fn _key(&self) -> String {
        format!("{}pl/{}/lock", self.prefix, self.place_id.0)
    }

    // Acquire the lock for this place.
    // Returns a mutex guard, which holds PlaceLockData (i.e. fencing token, revision)
    // Interactions with the local net should wait for this revision before doing anything.
    // All changes of the etcd state regarding this place should be safeguarded using the fencing
    // token
    pub async fn acquire(&self) -> Result<MutexGuard<PlaceLockData>> {
        let mut value = self.value.lock().await;
        if value.fencing_token.0.is_empty() {
            let resp = value
                .lock_client
                .lock(self._key(), Some(LockOptions::new().with_lease(self.lease.0)))
                .await?;
            value.fencing_token = resp.key().into();
            info!(
                "Acquired lock for place {} with key {} (=fencing token)",
                self.place_id.0,
                from_utf8(&value.fencing_token.0).unwrap_or("<not a valid utf8 str>")
            );
            value.min_revision =
                resp.header().expect("Header missing from etcd response").revision().into()
        }
        Ok(value)
    }

    pub async fn external_acquire(&self) -> Result<()> {
        let mut value = self.value.lock().await;
        if !value.fencing_token.0.is_empty() {
            // Note: Error will result in shutdown of the runner, revoking its lease and releasing
            // all of its locks.
            let fencing_token = std::mem::take(&mut value.fencing_token);
            let _ = value.lock_client.unlock(fencing_token.0).await?;
        }
        Ok(())
    }
}

impl PlaceLockData {
    pub fn min_revision(&self) -> Revision {
        self.min_revision
    }

    pub fn set_min_revision(&mut self, value: Revision) -> Revision {
        self.min_revision = max(self.min_revision, value);
        self.min_revision
    }

    pub fn fencing_token(&self) -> &FencingToken {
        &self.fencing_token
    }
}
