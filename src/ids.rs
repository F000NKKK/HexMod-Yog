//! Hex Casting's ids the code names: tags (`HexTags`), damage types,
//! features, startup grants.

/// Tags (`data/hexcasting/tags/…`).
pub mod tags {
    pub const CANNOT_TELEPORT: &str = "hexcasting:cannot_teleport";
    pub const STICKY_TELEPORTERS: &str = "hexcasting:sticky_teleporters";
    pub const CANNOT_BRAINSWEEP: &str = "hexcasting:cannot_brainsweep";
    pub const CHEAP_TO_BREAK_BLOCK: &str = "hexcasting:cheap_to_break_block";
    pub const WATER_PLANTS: &str = "hexcasting:water_plants";
    pub const PHIAL_BASE: &str = "hexcasting:phial_base";
}

/// `HexDamageTypes`.
pub mod damage {
    pub const OVERCAST: &str = "hexcasting:overcast";
}

/// The edified trees' configured features (`HexConfiguredFeatures`).
pub mod features {
    pub const AMETHYST_EDIFIED_TREE: &str = "hexcasting:amethyst_edified_tree";
    pub const AVENTURINE_EDIFIED_TREE: &str = "hexcasting:aventurine_edified_tree";
    pub const CITRINE_EDIFIED_TREE: &str = "hexcasting:citrine_edified_tree";
}

/// What a player gets on first joining.
pub mod grants {
    pub const HEXBOOK: &str = "hexcasting:give_hexbook";
    pub const STAFF: &str = "hexcasting:give_staff";
}

/// Item ids by prefix.
pub mod prefixes {
    pub const STAFF: &str = "hexcasting:staff/";
    pub const DYE_PIGMENT: &str = "hexcasting:dye_colorizer_";
    pub const NAMESPACE: &str = "hexcasting:";
}
