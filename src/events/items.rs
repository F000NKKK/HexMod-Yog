//! Right-clicking casting items (`ItemPackagedHex.use`) and scrolling the
//! spellbook and abacus (`MsgShiftScrollC2S`).

use yog_api::Server;

use crate::casting::env::{random, CastEnv, Caster, Source, Spray};
use crate::casting::vm::Vm;
use crate::item::holder::{self, ABACUS, SPELLBOOK};
use crate::item::{media, packaged};
use crate::math::Vec3;
use crate::protocol::Hand;
use crate::text::Text;
use crate::wire::Reader;

/// `ItemPackagedHex.use`, for the hand holding `item_id`.
pub fn use_packaged(srv: &dyn Server, player: &str, item_id: &str) {
    let Some(caster) = Caster::by_name(srv, player) else { return };
    let Some(probe) = CastEnv::for_player(srv, caster.clone(), Hand::Main, Source::Packaged) else { return };
    let Some(hand) = [Hand::Main, Hand::Off].into_iter().find(|&h| probe.held(h).is_some_and(|s| s.id == item_id))
    else {
        return;
    };
    let Some(stack) = probe.held(hand) else { return };
    let Some(instrs) = packaged::hex(&stack) else { return };
    let Some(env) = CastEnv::for_player(srv, caster, hand, Source::Packaged) else { return };
    let mut vm = Vm::new(Default::default(), env);
    let view = vm.queue_execute(instrs);
    let env = vm.env;

    let Some(pos) = env.caster_pos() else { return };
    if view.kind.success() {
        let spray = Spray { pos, vel: Vec3::new(0.0, 1.5, 0.0), fuzziness: 0.4, spread: std::f64::consts::PI / 3.0, count: 30 };
        env.spray(&spray);
    }
    if let Some(id) = env.loudest.id() {
        srv.play_sound(&env.dim, pos.x, pos.y, pos.z, id, 1.0, 1.0);
    }
    if let Some(mut stack) = env.held(hand) {
        if stack.id == item_id && media::breaks_after_depletion(&stack.id) && media::media(&stack) == 0 {
            stack.count -= 1;
            env.save(&stack);
            srv.play_sound(&env.dim, pos.x, pos.y, pos.z, "minecraft:entity.item.break", 0.8, 0.8 + random() as f32 * 0.4);
        }
    }
}

/// `MsgShiftScrollC2S.handle`: `num` main-hand delta, `num` off-hand delta,
/// `int` whether the sprint key is down.
pub fn shift_scroll(srv: &dyn Server, player: &str, payload: &[u8]) {
    let Ok(s) = std::str::from_utf8(payload) else { return };
    let mut r = Reader::new(s);
    let (Some(main), Some(off), Some(ctrl)) = (r.num(), r.num(), r.int()) else { return };
    let Some(caster) = Caster::by_name(srv, player) else { return };
    let Some(env) = CastEnv::for_player(srv, caster, Hand::Main, Source::Staff) else { return };
    for (hand, delta) in [(Hand::Main, main), (Hand::Off, off)] {
        if delta == 0.0 {
            continue;
        }
        let Some(mut stack) = env.held(hand) else { continue };
        let msg = match stack.id.as_str() {
            ABACUS => {
                let pitch = holder::abacus_scroll(&mut stack.nbt, delta, hand == Hand::Main, ctrl != 0);
                let pos = env.caster_pos().unwrap_or_default();
                srv.play_sound(&env.dim, pos.x, pos.y, pos.z, "hexcasting:abacus", 0.5, pitch);
                let v = stack.nbt.get_f64("value").unwrap_or(0.0);
                Text::key_args("hexcasting.tooltip.abacus", vec![Text::num(v)]).to_json_colored("green")
            }
            SPELLBOOK => {
                holder::sync_page_name(&mut stack.nbt);
                let idx = holder::rotate_page(&mut stack.nbt, delta < 0.0);
                let len = holder::highest_page(&stack.nbt);
                // Arguments as JSON components: the page's name is one already.
                let mut args = vec![Text::int(idx).to_json_colored("white"), Text::int(len).to_json_colored("white")];
                let name = stack.nbt.get("display").and_then(|d| d.get_str("Name")).map(str::to_owned);
                let mut key = "hexcasting.tooltip.spellbook.page".to_owned();
                if let Some(json) = name.filter(|_| hand == Hand::Off) {
                    key = "hexcasting.tooltip.spellbook.page_with_name".to_owned();
                    args.push(format!("{{\"text\":\"\",\"italic\":true,\"extra\":[{json}]}}"));
                }
                if holder::page_sealed(&stack.nbt) {
                    key.push_str(".sealed");
                    args.push(Text::key("hexcasting.tooltip.spellbook.sealed").to_json_colored("gold"));
                }
                format!("{{\"translate\":\"{key}\",\"color\":\"gray\",\"with\":[{}]}}", args.join(","))
            }
            _ => continue,
        };
        env.save(&stack);
        actionbar(srv, player, &msg);
    }
}

/// `ItemAbacus.use` while sneaking: back to zero.
pub fn reset_abacus(srv: &dyn Server, player: &str) {
    let Some(caster) = Caster::by_name(srv, player) else { return };
    let Some(env) = CastEnv::for_player(srv, caster, Hand::Main, Source::Staff) else { return };
    let Some((mut stack, _)) =
        [Hand::Main, Hand::Off].into_iter().filter_map(|h| Some((env.held(h)?, h))).find(|(s, _)| s.id == ABACUS)
    else {
        return;
    };
    let old = stack.nbt.remove("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
    env.save(&stack);
    srv.play_sound_to_player(player, "hexcasting:abacus.shake", 1.0, 1.0);
    let key = if old == 69.0 { "hexcasting.tooltip.abacus.reset.nice" } else { "hexcasting.tooltip.abacus.reset" };
    actionbar(srv, player, &Text::key(key).to_json());
}

/// `displayClientMessage(text, true)`.
fn actionbar(srv: &dyn Server, player: &str, json: &str) {
    srv.tellraw(player, json); // TODO(loader): actionbar_json
}
