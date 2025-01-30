use std::cmp::max;

use tokio::sync::{Mutex, MutexGuard};

use crate::{
    error::Result,
    net::{PlaceId, Revision},
    storage,
};

use crate::storage::FencingToken;
pub struct InMemoryPlaceLock {
    value: Mutex<InMemoryPlaceLockData>,
    place_id: PlaceId,
}

pub struct InMemoryPlaceLockData {
    fencing_token: FencingToken,
    min_revision: Revision,
}

impl InMemoryPlaceLock {
    pub fn new(place_id: PlaceId) -> Self {
        let value = Mutex::new(InMemoryPlaceLockData {
            fencing_token: Default::default(),
            min_revision: Default::default(),
        });
        InMemoryPlaceLock { value, place_id }
    }
}

impl storage::traits::PlaceLockClient for InMemoryPlaceLock {
    type PlaceLockData = InMemoryPlaceLockData;
    fn place_id(&self) -> PlaceId {
        self.place_id
    }

    /// Acquire the lock for this place.
    /// Returns a mutex guard, which holds PlaceLockData (i.e. fencing token, revision)
    /// Interactions with the local net should wait for this revision before doing anything.
    /// All changes of the etcd state regarding this place should be safeguarded using the fencing
    /// token
    async fn acquire(&self) -> Result<MutexGuard<InMemoryPlaceLockData>> {
        let mut value = self.value.lock().await;
        value.fencing_token = FencingToken(vec![]);
        Ok(value)
    }

    async fn external_acquire(&self) -> Result<()> {
        panic!("This is an in memory place lock. It should never be acquired externally (i.e. from another process). How does that even happen?");
    }
}

impl storage::traits::PlaceLockData for InMemoryPlaceLockData {
    fn min_revision(&self) -> Revision {
        self.min_revision
    }

    fn set_min_revision(&mut self, value: Revision) -> Revision {
        self.min_revision = max(self.min_revision, value);
        self.min_revision
    }

    fn fencing_token(&self) -> &FencingToken {
        &self.fencing_token
    }
}
