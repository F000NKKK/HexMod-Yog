//! Casting with a staff (Hex Casting's `ItemStaff.use` and
//! `StaffCastEnv.handleNewPatternOnServer`): the grid's patterns and the
//! VM's image live on the server, per player, between casts.

use yog_api::Server;

use crate::casting::env::{player_storage, CastEnv, Caster, Source};
use crate::casting::image::Image;
use crate::casting::vm::Vm;
use crate::iota::Iota;
use crate::protocol::{
    read_patterns, write_patterns, Hand, NewPattern, OpenGui, PatternResult, ResolvedPattern, CH_OPEN_GUI,
    CH_PATTERN_RESULT,
};
use crate::wire::{Reader, Writer};

const KEY_IMAGE: &str = "staff.image";
const KEY_PATTERNS: &str = "staff.patterns";

pub fn is_staff(id: &str) -> bool {
    id.starts_with("hexcasting:staff/")
}

/// `getStaffcastVM`'s image.
pub fn image(srv: &dyn Server, uuid: &str) -> Image {
    player_storage(srv, uuid).get_str(KEY_IMAGE).map(Image::decode).unwrap_or_default()
}

/// `getPatternsSavedInUi`.
pub fn patterns(srv: &dyn Server, uuid: &str) -> Vec<ResolvedPattern> {
    let s = player_storage(srv, uuid);
    s.get_str(KEY_PATTERNS).and_then(|p| read_patterns(&mut Reader::new(p))).unwrap_or_default()
}

fn save(srv: &dyn Server, uuid: &str, image: Option<&Image>, pats: &[ResolvedPattern]) {
    let mut s = player_storage(srv, uuid);
    match image {
        Some(img) => s.set(KEY_IMAGE, img.encode()),
        None => {
            s.remove(KEY_IMAGE);
        }
    }
    if pats.is_empty() {
        s.remove(KEY_PATTERNS);
    } else {
        let mut w = Writer::new();
        write_patterns(&mut w, pats);
        s.set(KEY_PATTERNS, w.finish());
    }
}

/// `clearCastingData`.
pub fn clear(srv: &dyn Server, uuid: &str) {
    save(srv, uuid, None, &[]);
}

/// The hand holding a staff (the main hand first, as the game tries it).
fn staff_hand(srv: &dyn Server, player: &str) -> Option<Hand> {
    let caster = Caster::by_name(srv, player)?;
    let env = CastEnv::for_player(srv, caster, Hand::Main, Source::Staff)?;
    [Hand::Main, Hand::Off].into_iter().find(|&h| env.held(h).is_some_and(|s| is_staff(&s.id)))
}

/// `ItemStaff.use`: sneaking clears the grid; then the grid opens with
/// what's saved.
pub fn use_staff(srv: &dyn Server, player: &str, sneaking: bool) {
    let Some(info) = srv.player_info(player) else { return };
    let Some(hand) = staff_hand(srv, player) else { return };
    if sneaking {
        clear(srv, &info.uuid);
        srv.play_sound_to_player(player, "hexcasting:staff.reset", 1.0, 1.0);
    }
    let image = image(srv, &info.uuid);
    let gui = OpenGui {
        hand,
        patterns: patterns(srv, &info.uuid),
        stack: image.stack.clone(),
        ravenmind: image.ravenmind.clone(),
        paren_count: image.paren_count,
    };
    srv.send_to_player(player, CH_OPEN_GUI, &gui.encode());
}

/// `handleNewPatternOnServer`.
pub fn new_pattern(srv: &dyn Server, player: &str, payload: &[u8]) {
    let Some(msg) = NewPattern::decode(payload) else { return };
    let Some(caster) = Caster::by_name(srv, player) else { return };
    let uuid = caster.uuid.clone();
    let Some(env) = CastEnv::for_player(srv, caster, msg.hand, Source::Staff) else { return };
    if !env.held(msg.hand).is_some_and(|s| is_staff(&s.id)) {
        return;
    }
    let mut vm = Vm::new(image(srv, &uuid), env);
    let view = vm.queue_execute(vec![Iota::Pattern(msg.pattern)]);
    let mut resolved = msg.resolved;
    if view.stack_clear {
        save(srv, &uuid, None, &[]);
    } else {
        if let Some(last) = resolved.last_mut() {
            last.kind = view.kind;
        }
        save(srv, &uuid, Some(&vm.image), &resolved);
    }
    let res = PatternResult {
        stack_clear: view.stack_clear,
        kind: view.kind,
        stack: view.stack,
        ravenmind: view.ravenmind,
        index: resolved.len() as i64 - 1,
    };
    srv.send_to_player(player, CH_PATTERN_RESULT, &res.encode());
}
