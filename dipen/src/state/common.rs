#[derive(Eq, PartialEq, Clone, Copy, PartialOrd, Ord, Hash, Debug)]
pub struct LeaseId(pub i64);

#[derive(Eq, PartialEq, Clone, Copy, PartialOrd, Ord, Hash, Debug)]
pub struct Version(pub u64);

#[derive(Eq, PartialEq, Clone, PartialOrd, Ord, Debug, Default)]
pub struct FencingToken(pub Vec<u8>);

impl From<i64> for Version {
    fn from(value: i64) -> Self {
        // Note: versions are not negative, no idea why etcd uses i64 here
        Version(value as u64)
    }
}

impl From<i64> for LeaseId {
    fn from(value: i64) -> Self {
        LeaseId(value)
    }
}

impl From<Vec<u8>> for FencingToken {
    fn from(value: Vec<u8>) -> Self {
        FencingToken(value)
    }
}

impl From<&[u8]> for FencingToken {
    fn from(value: &[u8]) -> Self {
        FencingToken(value.to_vec())
    }
}
