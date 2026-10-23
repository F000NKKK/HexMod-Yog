//! Casting with a staff (Hex Casting's `ItemStaff.use` and
//! `StaffCastEnv.handleNewPatternOnServer`): the grid's patterns and the
//! VM's image live on the server, per player, between casts.

use crate::casting::env::{CastEnv, Caster, Source};
use crate::casting::image::Image;
use crate::casting::vm::Vm;
use crate::iota::Iota;
use crate::protocol::{
    CH_OPEN_GUI, CH_PATTERN_RESULT, Hand, NewPattern, OpenGui, PatternResult, ResolvedPattern,
    read_patterns, write_patterns,
};
use crate::wire::{Reader, Writer};

/// A player's staff grid as they left it (its image and drawn patterns,
/// in the casting's own text forms), kept with the world.
#[derive(Default, yog_api::Schema)]
struct Grid {
    image: Option<String>,
    patterns: Option<String>,
}

static GRID: yog_api::storage::PlayerData<Grid> = yog_api::storage::PlayerData::new("staff");

pub fn is_staff(id: &str) -> bool {
    id.starts_with(crate::ids::prefixes::STAFF)
}

/// `getStaffcastVM`'s image.
pub fn image(uuid: &str) -> Image {
    GRID.with(uuid, |g| g.image.as_deref().map(Image::decode))
        .unwrap_or_default()
}

/// `getPatternsSavedInUi`.
pub fn patterns(uuid: &str) -> Vec<ResolvedPattern> {
    GRID.with(uuid, |g| {
        g.patterns
            .as_deref()
            .and_then(|p| read_patterns(&mut Reader::new(p)))
    })
    .unwrap_or_default()
}

fn save(uuid: &str, image: Option<&Image>, pats: &[ResolvedPattern]) {
    let patterns = (!pats.is_empty()).then(|| {
        let mut w = Writer::new();
        write_patterns(&mut w, pats);
        w.finish()
    });
    GRID.with_mut(uuid, |g| {
        g.image = image.map(Image::encode);
        g.patterns = patterns;
    });
}

/// `clearCastingData`.
pub fn clear(uuid: &str) {
    save(uuid, None, &[]);
}

/// The hand holding a staff (the main hand first, as the game tries it).
fn staff_hand(player: &str) -> Option<Hand> {
    let caster = Caster::by_name(player)?;
    let env = CastEnv::for_player(caster, Hand::Main, Source::Staff)?;
    [Hand::Main, Hand::Off]
        .into_iter()
        .find(|&h| env.held(h).is_some_and(|s| is_staff(&s.id)))
}

/// `ItemStaff.use`: sneaking clears the grid; then the grid opens with
/// what's saved.
pub fn use_staff(player: &str, sneaking: bool) {
    let Some(info) = yog_api::event::player::info(player) else {
        return;
    };
    let Some(hand) = staff_hand(player) else {
        return;
    };
    if sneaking {
        clear(&info.uuid);
        yog_api::event::sound::play_to_player(player, crate::res::SoundEvents::STAFF_RESET, 1.0, 1.0);
    }
    let image = image(&info.uuid);
    let gui = OpenGui {
        hand,
        patterns: patterns(&info.uuid),
        stack: image.stack.clone(),
        ravenmind: image.ravenmind.clone(),
        paren_count: image.paren_count,
    };
    yog_api::event::net::send_to_player(player, CH_OPEN_GUI, gui.encode());
}

/// `handleNewPatternOnServer`.
pub fn new_pattern(player: &str, payload: &[u8]) {
    let Some(msg) = NewPattern::decode(payload) else {
        return;
    };
    let Some(caster) = Caster::by_name(player) else {
        return;
    };
    let uuid = caster.uuid.clone();
    let Some(env) = CastEnv::for_player(caster, msg.hand, Source::Staff) else {
        return;
    };
    if !env.held(msg.hand).is_some_and(|s| is_staff(&s.id)) {
        return;
    }
    let mut vm = Vm::new(image(&uuid), env);
    let view = vm.queue_execute(vec![Iota::Pattern(msg.pattern)]);
    let mut resolved = msg.resolved;
    if view.stack_clear {
        save(&uuid, None, &[]);
    } else {
        if let Some(last) = resolved.last_mut() {
            last.kind = view.kind;
        }
        save(&uuid, Some(&vm.image), &resolved);
    }
    let res = PatternResult {
        stack_clear: view.stack_clear,
        kind: view.kind,
        stack: view.stack,
        ravenmind: view.ravenmind,
        index: resolved.len() as i64 - 1,
    };
    yog_api::event::net::send_to_player(player, CH_PATTERN_RESULT, res.encode());
}
