use crate::exec::{StartContext, StartTakenTokenContext, StartTokenContext};
use crate::net::{PetriNet, PlaceId, TokenId, TransitionId};

pub struct StartContextStruct<'a> {
    net: &'a PetriNet,
    transition_id: TransitionId,
}

pub struct StartTokenContextStruct<'a> {
    net: &'a PetriNet,
    token_id: TokenId,
    place_id: PlaceId,
}

pub struct StartTakenTokenContextStruct<'a> {
    net: &'a PetriNet,
    token_id: TokenId,
    transition_id: TransitionId,
    place_id: PlaceId,
}

impl<'a> StartContextStruct<'a> {
    pub fn new(net: &'a PetriNet, transition_id: TransitionId) -> Self {
        Self { net, transition_id }
    }
}
impl<'a> StartContext for StartContextStruct<'a> {
    fn tokens_at(
        &self,
        place_id: PlaceId,
    ) -> impl Iterator<Item = impl crate::exec::StartTokenContext> {
        let net = self.net;
        net.place(place_id)
            .unwrap()
            .token_ids()
            .iter()
            .map(move |&token_id| StartTokenContextStruct { net, token_id, place_id })
    }

    fn taken_tokens_at(
        &self,
        place_id: PlaceId,
    ) -> impl Iterator<Item = impl crate::exec::StartTakenTokenContext> {
        let net = self.net;
        net.place(place_id).unwrap().taken_token_ids().iter().map(
            move |(&token_id, &transition_id)| StartTakenTokenContextStruct {
                net,
                token_id,
                transition_id,
                place_id,
            },
        )
    }

    fn tokens(&self) -> impl Iterator<Item = impl StartTokenContext> {
        let net = self.net;
        net.arcs_for(self.transition_id).flat_map(move |(pl_id, _)| self.tokens_at(pl_id))
    }

    fn taken_tokens(&self) -> impl Iterator<Item = impl StartTakenTokenContext> {
        let net = self.net;
        net.arcs_for(self.transition_id).flat_map(move |(pl_id, _)| self.taken_tokens_at(pl_id))
    }
}

impl StartTokenContext for StartTokenContextStruct<'_> {
    fn token_id(&self) -> TokenId {
        self.token_id
    }

    fn data(&self) -> &[u8] {
        self.net.token(self.token_id).unwrap().data()
    }

    fn place_id(&self) -> PlaceId {
        self.place_id
    }
}

impl StartTakenTokenContext for StartTakenTokenContextStruct<'_> {
    fn token_id(&self) -> TokenId {
        self.token_id
    }

    fn data(&self) -> &[u8] {
        self.net.token(self.token_id).unwrap().data()
    }

    fn place_id(&self) -> PlaceId {
        self.place_id
    }

    fn transition_id(&self) -> TransitionId {
        self.transition_id
    }
}
