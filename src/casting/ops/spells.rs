//! Spells (`casting/actions/spells/*`).
use crate::casting::actions::{actions, Action};

pub fn actions() -> Vec<Action> {
    actions! {}
}

/// The flight a player was granted (`OpFlight`), if any.
pub fn flight_of(_srv: &dyn yog_api::Server, _uuid: &str) -> Option<()> {
    None
}
