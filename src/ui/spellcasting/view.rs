//! The generated screen (`ID`, `register`, `close`, …), apart so its
//! `open()` doesn't clash with [`super::open`].

use super::SpellcastingVm;
yog_api::ui_include!("ui/spellcasting");
