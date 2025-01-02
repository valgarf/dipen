use crate::exec::{RunContext, RunTokenContext};
use crate::net::{PlaceId, TokenId};

pub struct RunContextStruct {
    pub tokens: Vec<RunTokenContextStruct>,
}

pub struct RunTokenContextStruct {
    pub token_id: TokenId,
    pub orig_place_id: PlaceId,
    pub data: Vec<u8>,
}

impl RunContext for RunContextStruct {
    fn tokens(&self) -> impl Iterator<Item = &impl crate::exec::RunTokenContext> {
        self.tokens.iter()
    }
}

impl RunTokenContext for RunTokenContextStruct {
    fn token_id(&self) -> TokenId {
        self.token_id
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn orig_place_id(&self) -> PlaceId {
        self.orig_place_id
    }
}
