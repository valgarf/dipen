use tokio::sync::MutexGuard;

use crate::net::{PlaceId, Revision};

use super::FencingToken;
use crate::error::Result;

pub trait PlaceLockData {
    fn min_revision(&self) -> Revision;
    fn set_min_revision(&mut self, value: Revision) -> Revision;
    fn fencing_token(&self) -> &FencingToken;
}

pub trait PlaceLock {
    type PlaceLockData: PlaceLockData;
    fn place_id(&self) -> PlaceId;
    fn acquire(
        &self,
    ) -> impl std::future::Future<Output = Result<MutexGuard<Self::PlaceLockData>>> + Send;
    fn external_acquire(&self) -> impl std::future::Future<Output = Result<()>> + Send;
}
