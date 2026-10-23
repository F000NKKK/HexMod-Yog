//! Right-clicking casting items (`ItemPackagedHex.use`) and scrolling the
//! spellbook and abacus (`MsgShiftScrollC2S`).

use crate::casting::env::{CastEnv, Caster, Source, Spray, random};
use crate::casting::vm::Vm;
use crate::content::{Advancements, Items};
use crate::item::holder::{self, ABACUS, SPELLBOOK};
use crate::item::{media, packaged};
use crate::protocol::Hand;
use crate::res::{self, Lang};
use crate::text::Text;
use crate::wire::Reader;

/// The abacus' click.
const SOUND_ABACUS: &str = crate::res::SoundEvents::ABACUS;
/// Its reset.
const SOUND_ABACUS_SHAKE: &str = crate::res::SoundEvents::ABACUS_SHAKE;

/// `ItemPackagedHex.use`, for the hand holding `item_id`.
pub fn use_packaged(player: &str, item_id: &str) {
    let Some(caster) = Caster::by_name(player) else {
        return;
    };
    let Some(probe) = CastEnv::for_player(caster.clone(), Hand::Main, Source::Packaged) else {
        return;
    };
    let Some(hand) = [Hand::Main, Hand::Off]
        .into_iter()
        .find(|&h| probe.held(h).is_some_and(|s| s.id == item_id))
    else {
        return;
    };
    let Some(stack) = probe.held(hand) else {
        return;
    };
    let Some(instrs) = packaged::hex(&stack) else {
        return;
    };
    let Some(env) = CastEnv::for_player(caster, hand, Source::Packaged) else {
        return;
    };
    let mut vm = Vm::new(Default::default(), env);
    let view = vm.queue_execute(instrs);
    let env = vm.env;
    yog_api::event::player::set_item_cooldown(player, item_id, media::cooldown(item_id) as i32);

    let Some(pos) = env.caster_pos() else { return };
    if view.kind.success() {
        let spray = Spray { pos, ..CAST_SPRAY };
        env.spray(&spray);
    }
    if let Some(id) = env.loudest.id() {
        yog_api::event::sound::play(&env.dim, (pos.x, pos.y, pos.z), id, 1.0, 1.0);
    }
    if let Some(mut stack) = env.held(hand)
        && stack.id == item_id
        && media::breaks_after_depletion(&stack.id)
        && media::media(&stack) == 0
    {
        stack.count -= 1;
        env.save(&stack);
        yog_api::event::sound::play(
            &env.dim,
            (pos.x, pos.y, pos.z),
            yog_api::vanilla::sounds::ENTITY_ITEM_BREAK,
            BREAK_SOUND.0,
            BREAK_SOUND.1 + random() as f32 * BREAK_SOUND.2,
        );
    }
}

/// `MsgShiftScrollC2S.handle`: `num` main-hand delta, `num` off-hand delta,
/// `int` whether the sprint key is down.
pub fn shift_scroll(player: &str, payload: &[u8]) {
    let Ok(s) = std::str::from_utf8(payload) else {
        return;
    };
    let mut r = Reader::new(s);
    let (Some(main), Some(off), Some(ctrl)) = (r.num(), r.num(), r.int()) else {
        return;
    };
    let Some(caster) = Caster::by_name(player) else {
        return;
    };
    let Some(env) = CastEnv::for_player(caster, Hand::Main, Source::Staff) else {
        return;
    };
    for (hand, delta) in [(Hand::Main, main), (Hand::Off, off)] {
        if delta == 0.0 {
            continue;
        }
        let Some(mut stack) = env.held(hand) else {
            continue;
        };
        let msg = match stack.id.as_str() {
            ABACUS => {
                let pitch =
                    holder::abacus_scroll(&mut stack.nbt, delta, hand == Hand::Main, ctrl != 0);
                let pos = env.caster_pos().unwrap_or_default();
                yog_api::event::sound::play(
                    &env.dim,
                    (pos.x, pos.y, pos.z),
                    SOUND_ABACUS,
                    0.5,
                    pitch,
                );
                let v = stack.nbt.get_f64("value").unwrap_or(0.0);
                Text::key_args(Lang::TOOLTIP_ABACUS_KEY, vec![Text::num(v)])
                    .to_json_colored("green")
            }
            SPELLBOOK => {
                holder::sync_page_name(&mut stack.nbt);
                let idx = holder::rotate_page(&mut stack.nbt, delta < 0.0);
                let len = holder::highest_page(&stack.nbt);
                // Arguments as JSON components: the page's name is one already.
                let mut args = vec![
                    Text::int(idx).to_json_colored("white"),
                    Text::int(len).to_json_colored("white"),
                ];
                let name = stack
                    .nbt
                    .get("display")
                    .and_then(|d| d.get_str("Name"))
                    .map(str::to_owned);
                let mut key = Lang::TOOLTIP_SPELLBOOK_PAGE_KEY.to_owned();
                if let Some(json) = name.filter(|_| hand == Hand::Off) {
                    key = Lang::TOOLTIP_SPELLBOOK_PAGE_WITH_NAME_KEY.to_owned();
                    args.push(format!(
                        "{{\"text\":\"\",\"italic\":true,\"extra\":[{json}]}}"
                    ));
                }
                if holder::page_sealed(&stack.nbt) {
                    key.push_str(".sealed");
                    args.push(
                        Text::key(Lang::TOOLTIP_SPELLBOOK_SEALED_KEY).to_json_colored("gold"),
                    );
                }
                format!(
                    "{{\"translate\":\"{key}\",\"color\":\"gray\",\"with\":[{}]}}",
                    args.join(",")
                )
            }
            _ => continue,
        };
        env.save(&stack);
        actionbar(player, &msg);
    }
}

/// `ItemAbacus.use` while sneaking: back to zero.
pub fn reset_abacus(player: &str) {
    let Some(caster) = Caster::by_name(player) else {
        return;
    };
    let Some(env) = CastEnv::for_player(caster, Hand::Main, Source::Staff) else {
        return;
    };
    let Some((mut stack, _)) = [Hand::Main, Hand::Off]
        .into_iter()
        .filter_map(|h| Some((env.held(h)?, h)))
        .find(|(s, _)| s.id == ABACUS)
    else {
        return;
    };
    let old = stack
        .nbt
        .remove("value")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    env.save(&stack);
    yog_api::event::sound::play_to_player(player, SOUND_ABACUS_SHAKE, 1.0, 1.0);
    let key = if old == NICE {
        Lang::TOOLTIP_ABACUS_RESET_NICE_KEY
    } else {
        Lang::TOOLTIP_ABACUS_RESET_KEY
    };
    actionbar(player, &Text::key(key).to_json());
}

/// The lore a fragment can tell (`ItemLoreFragment.NAMES`).
const LORE: [&str; 8] = [
    Advancements::LORE_CARDAMOM1,
    Advancements::LORE_CARDAMOM2,
    Advancements::LORE_CARDAMOM3,
    Advancements::LORE_CARDAMOM4,
    Advancements::LORE_CARDAMOM5,
    Advancements::LORE_EXPERIMENT1,
    Advancements::LORE_EXPERIMENT2,
    Advancements::LORE_INVENTORY,
];
/// What a fragment gives once all the lore is found (`giveExperiencePoints`).
const LORE_FOUND_XP: i32 = 20;
/// The spray of a cast from an item, the sound of one used up (its
/// volume, its pitch and how much higher it may go), and the abacus'
/// nicest number.
const CAST_SPRAY: Spray = Spray::look((0.0, 1.5, 0.0), 0.4, std::f64::consts::PI / 3.0, 30);
const BREAK_SOUND: (f32, f32, f32) = (0.8, 0.8, 0.4);
const NICE: f64 = 69.0;

/// `ItemLoreFragment.use`: a piece of lore not read yet, picked at random
/// — or, all of it read, some experience. The fragment is used up.
pub fn read_lore(player: &str) {
    let Some(caster) = Caster::by_name(player) else {
        return;
    };
    let Some(env) = CastEnv::for_player(caster, Hand::Main, Source::Staff) else {
        return;
    };
    let Some(mut stack) = [Hand::Main, Hand::Off]
        .into_iter()
        .filter_map(|h| env.held(h))
        .find(|s| s.id == Items::LORE_FRAGMENT)
    else {
        return;
    };
    yog_api::event::sound::play_to_player(
        player,
        crate::res::SoundEvents::LORE_FRAGMENT_READ,
        1.0,
        1.0,
    );
    let unread: Vec<&str> = LORE
        .into_iter()
        .filter(|id| !env.has_advancement(id))
        .collect();
    if unread.is_empty() {
        actionbar(
            player,
            &Text::key(res::Items::LORE_FRAGMENT_ALL_KEY).to_json(),
        );
        yog_api::event::player::add_xp(player, LORE_FOUND_XP);
    } else {
        let pick = ((random() * unread.len() as f64) as usize).min(unread.len() - 1);
        env.grant_advancement(Advancements::LORE);
        env.grant_advancement(unread[pick]);
    }
    stack.count -= 1;
    env.save(&stack);
}

/// `displayClientMessage(text, true)`.
fn actionbar(player: &str, json: &str) {
    yog_api::event::chat::actionbar_json(player, json);
}
