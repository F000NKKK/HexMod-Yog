//! Action registry and action definitions.

use crate::casting::SpellList;
use crate::math::HexPattern;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionId(pub &'static str);

pub trait Action: std::fmt::Debug {
    fn execute(&self, args: &SpellList) -> CastResult;
    fn get_pattern(&self) -> &HexPattern;
    fn get_id(&self) -> ActionId;
}

#[derive(Debug, Clone, PartialEq)]
pub struct CastResult {
    pub side_effects: Vec<SideEffect>,
    pub new_image: Option<SpellList>,
    pub pattern_type: ResolvedPatternType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedPatternType {
    Execute,
    Escape,
    Invalid,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SideEffect {}

#[derive(Debug, Default)]
pub struct ActionRegistry {
    actions: HashMap<ActionId, Box<dyn Action>>,
}

impl ActionRegistry {
    pub fn register(&mut self, action: Box<dyn Action>) {
        self.actions.insert(action.get_id(), action);
    }

    pub fn get(&self, id: ActionId) -> Option<&dyn Action> {
        self.actions.get(&id).map(|a| a.as_ref())
    }

    pub fn all(&self) -> Vec<&dyn Action> {
        self.actions.values().map(|a| a.as_ref()).collect()
    }
}