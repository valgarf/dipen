use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use tracing::debug;

use crate::error::{PetriError, Result};

use super::{Arc, PetriNet, Place, PlaceId, Transition, TransitionId};

#[derive(Default, Clone)]
pub struct PetriNetBuilder {
    places: HashMap<String, Place>,
    transitions: HashMap<String, Transition>,
    arcs: HashMap<(String, String), Arc>, // key: (place name, transition name)
}

#[derive(Default)]
pub struct PetriNetIds {
    pub places: HashMap<String, PlaceId>,
    pub transitions: HashMap<String, TransitionId>,
}

impl PetriNetBuilder {
    pub fn load(_path: &Path) -> Result<PetriNetBuilder> {
        Err(PetriError::NotImplemented())
        // TODO
    }

    pub fn save(_path: &Path) -> Result<()> {
        Err(PetriError::NotImplemented())
        // TODO
    }

    pub fn transitions(&self) -> &HashMap<String, Transition> {
        &self.transitions
    }

    pub fn places(&self) -> &HashMap<String, Place> {
        &self.places
    }

    pub fn arcs(&self) -> &HashMap<(String, String), Arc> {
        &self.arcs
    }

    pub fn is_empty(&self) -> bool {
        self.places.is_empty() && self.transitions.is_empty() && self.arcs.is_empty()
    }

    /// Insert place into this petri net.
    ///
    /// Returns the existing place for this name, or None if the name is not in use.
    pub fn insert_place(&mut self, place: Place) -> Option<Place> {
        self.places.insert(place.name().to_string(), place)
    }

    /// Insert transition into this petri net.
    ///
    /// Returns the existing transition for this name, or None if the name is not in use.
    pub fn insert_transition(&mut self, transition: Transition) -> Option<Transition> {
        self.transitions.insert(transition.name().to_string(), transition)
    }

    /// Insert arc into this petri net.
    ///
    /// Returns the existing arc for this name, or None if the name is not in use.
    pub fn insert_arc(&mut self, arc: Arc) -> Result<Option<Arc>> {
        let key = (arc.place().to_string(), arc.transition().to_string());
        if !self.places.contains_key(arc.place()) {
            let place_name = arc.place();
            let transition_name = arc.transition();
            return Err(PetriError::ValueError(format!(
                "Arc '{place_name}' <-> '{transition_name}' cannot be added, place does not exist."
            )));
        };
        if !self.transitions.contains_key(arc.transition()) {
            let place_name = arc.place();
            let transition_name = arc.transition();
            return Err(PetriError::ValueError(format!(
                "Arc '{place_name}' <-> '{transition_name}' cannot be added, transition does not exist."
            )));
        };
        Ok(self.arcs.insert(key, arc))
    }

    /// Reduce the petri net to a single region
    ///
    /// The resulting PetriNetBuilder will only contain transitions from the given region and all
    /// places connected to these transitions.
    pub fn only_region<T: AsRef<str>>(&self, region: T) -> Result<Self> {
        let transition_names: HashSet<String> = self
            .transitions
            .values()
            .filter_map(|tr| {
                if tr.region() == region.as_ref() {
                    Some(tr.name().to_string())
                } else {
                    None
                }
            })
            .collect();

        let mut result = PetriNetBuilder::default();
        let mut place_names = HashSet::<&str>::new();
        for tr_name in &transition_names {
            result.insert_transition(self.transitions.get(tr_name).unwrap().clone());
            // Note: if these unwraps panic, we have a logic error here somewhere
        }
        for ((pl_name, tr_name), arc) in &self.arcs {
            if !transition_names.contains(tr_name) {
                continue;
            }
            if !place_names.contains(pl_name.as_str()) {
                result.insert_place(self.places.get(pl_name).unwrap().clone());
                // Note: if these unwraps panic, we have a logic error here somewhere
                place_names.insert(pl_name);
            }
            result.insert_arc(arc.clone()).unwrap();
            // Note: if this unwrap panics, we have a logic error here somewhere
        }
        let original_transitions = self.transitions.len();
        let original_places = self.places.len();
        let transitions = result.transitions.len();
        let places = result.places.len();
        let region = region.as_ref();
        debug!(
            region,
            original_transitions,
            original_places,
            transitions,
            places,
            "Constructed reduced net for region."
        );
        Ok(result)
    }

    /// Build the PetriNet
    ///
    /// Requires a mapping of place / transition names to ids.
    /// Place ids must be unique across all places in the net and transition ids must be unique
    /// across transitions in the net.
    ///
    /// For distributed running, the ids must be stored / retrieved from a central server.
    pub fn build(&self, ids: &PetriNetIds) -> Result<PetriNet> {
        let mut net = PetriNet::default();
        for (tr_name, tr_data) in &self.transitions {
            let tr_id = ids.transitions.get(tr_name).ok_or(PetriError::ValueError(format!(
                "Transition '{tr_name}' not found in provided ids."
            )))?;
            net.transitions.insert(*tr_id, tr_data.clone());
        }
        for (pl_name, pl_data) in &self.places {
            let pl_id = ids.places.get(pl_name).ok_or(PetriError::ValueError(format!(
                "Place '{pl_name}' not found in provided ids."
            )))?;
            net.places.insert(*pl_id, pl_data.clone());
        }
        for ((pl_name, tr_name), arc_data) in &self.arcs {
            let pl_id = ids.places.get(pl_name).ok_or(PetriError::ValueError(format!(
                "Place '{pl_name}' not found in provided ids."
            )))?;
            let tr_id = ids.transitions.get(tr_name).ok_or(PetriError::ValueError(format!(
                "Transition '{tr_name}' not found in provided ids."
            )))?;
            net.arcs.insert((*tr_id, *pl_id), arc_data.clone());
        }
        Ok(net)
    }
}
