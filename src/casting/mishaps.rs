//! Mishaps — errors that occur during casting.

use crate::iota::Iota;
use crate::math::HexPattern;

/// A mishap that occurred during spell execution.
#[derive(Debug, Clone)]
pub struct Mishap {
    pub name: &'static str,
    pub context: MishapContext,
    pub stack: Vec<Iota>,
}

#[derive(Debug, Clone)]
pub struct MishapContext {
    pub pattern: HexPattern,
    pub position: Option<usize>,
}

impl Mishap {
    pub fn new(name: &'static str, context: MishapContext) -> Self {
        Mishap {
            name,
            context,
            stack: Vec::new(),
        }
    }
}

// TODO: Define specific mishap types (MishapBadIota, MishapStackUnderflow, etc.)