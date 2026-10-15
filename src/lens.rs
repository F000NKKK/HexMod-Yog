//! The scrying lens (`ScryingLensOverlays`, `HexAdditionalRenderers
//! .tryRenderScryingLensOverlay`): held or worn, it shows what the block
//! looked at is up to — redstone, notes, an impetus' media and messages, a
//! bookshelf's iota.
//!
//! The server works the lines out (it has the impetuses and shelves) and
//! sends them to the player; the client draws them right of the crosshair.

use std::sync::Mutex;

use yog_api::{GfxContext, Registry, Server, split_state};

use crate::akashic;
use crate::casting::env::{SLOT_OFFHAND, Stack, from_ypos, look_vec, to_ypos};
use crate::circle;
use crate::content::{Blocks, Items};
use crate::item::media::DUST;
use crate::math::Vec3;
use crate::text::Text;
use crate::wire::{Reader, Writer};

/// Server → client: the lines to show (none: nothing).
pub const CH_LENS: &str = "hexcasting:lens";
/// How far the lens sees, as a hand reaches.
const REACH: f64 = 5.0;
/// Every so many ticks the lines are worked out again.
const EVERY: u64 = 2;
const HEAD_SLOT: u32 = 39;

/// A line: the item drawn beside it, its color (`None`: white) and text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub item: String,
    pub color: Option<u32>,
    pub text: Text,
}

fn line(item: &str, text: Text) -> Line {
    Line { item: item.into(), color: None, text }
}

fn colored(item: &str, color: u32, text: impl Into<String>) -> Line {
    Line { item: item.into(), color: Some(color), text: Text::lit(text) }
}

pub fn encode(lines: &[Line]) -> Vec<u8> {
    let mut w = Writer::new();
    w.int(lines.len() as i64);
    for l in lines {
        w.str(&l.item).int(l.color.map_or(-1, |c| c as i64));
        l.text.write(&mut w);
    }
    w.finish().into_bytes()
}

pub fn decode(b: &[u8]) -> Option<Vec<Line>> {
    let mut r = Reader::new(std::str::from_utf8(b).ok()?);
    let n = r.int()?;
    (0..n)
        .map(|_| {
            let item = r.str()?.to_owned();
            let color = r.int()?;
            let text = Text::read(&mut r)?;
            Some(Line { item, color: (color >= 0).then_some(color as u32), text })
        })
        .collect()
}

// ── what a block shows ──────────────────────────────────────────────────────

/// `RedStoneWireBlock.getColorForPower`.
fn redstone_color(power: i32, max: i32) -> u32 {
    let p = ((power * max) / 15).clamp(0, 15) as f32 / 15.0;
    let r = p * 0.6 + if p > 0.0 { 0.4 } else { 0.3 };
    let g = (p * p * 0.7 - 0.5).clamp(0.0, 1.0);
    let b = (p * p * 0.6 - 0.7).clamp(0.0, 1.0);
    ((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | (b * 255.0) as u32
}

const INSTRUMENTS: [&str; 23] = [
    "harp", "basedrum", "snare", "hat", "bass", "flute", "bell", "guitar", "chime", "xylophone", "iron_xylophone", "cow_bell",
    "didgeridoo", "bit", "banjo", "pling", "zombie", "skeleton", "creeper", "dragon", "wither_skeleton", "piglin", "custom_head",
];

/// `instrumentColor`: the instrument's block's map color.
fn instrument_color(name: &str) -> u32 {
    match name {
        "basedrum" => 0x707070,
        "snare" | "xylophone" | "pling" => 0xF7E9A3,
        "hat" => 0xFFFCF5,
        "bass" => 0x8F7748,
        "flute" => 0xA4A8B8,
        "bell" => 0xFAEE4D,
        "guitar" => 0xC7C7C7,
        "chime" => 0xA0A0FF,
        "iron_xylophone" => 0xA7A7A7,
        "cow_bell" => 0x664C33,
        "didgeridoo" => 0xD87F33,
        "bit" => 0x00D93A,
        "banjo" => 0xE5E533,
        _ => 0xFFFFFF,
    }
}

/// `DecimalFormat("###,###.##")`.
fn dust_amount(v: f64) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    let whole = rounded.trunc() as i64;
    let frac = ((rounded - whole as f64).abs() * 100.0).round() as i64;
    let digits = whole.abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if whole < 0 { "-" } else { "" };
    match frac {
        0 => format!("{sign}{grouped}"),
        f if f % 10 == 0 => format!("{sign}{grouped}.{}", f / 10),
        f => format!("{sign}{grouped}.{f:02}"),
    }
}

/// `PoweredRailBlock`'s signal search, as the lens repeats it.
fn powered_rail_strength(srv: &dyn Server, dim: &str, pos: (i32, i32, i32)) -> i32 {
    if srv.redstone_power(dim, to_ypos(pos)).is_some_and(|p| p > 0) {
        return 9;
    }
    let shape = srv.block_property(dim, to_ypos(pos), "shape").unwrap_or_default();
    rail_signal(srv, dim, pos, &shape, true, 0).max(rail_signal(srv, dim, pos, &shape, false, 0))
}

fn rail_signal(srv: &dyn Server, dim: &str, pos: (i32, i32, i32), shape: &str, positive: bool, depth: i32) -> i32 {
    if depth >= 8 {
        return 0;
    }
    let (mut x, mut y, mut z) = pos;
    let mut descending = true;
    let mut shape = shape.to_owned();
    match shape.as_str() {
        "north_south" => z += if positive { 1 } else { -1 },
        "east_west" => x += if positive { -1 } else { 1 },
        "ascending_east" => {
            if positive {
                x -= 1;
            } else {
                x += 1;
                y += 1;
                descending = false;
            }
            shape = "east_west".into();
        }
        "ascending_west" => {
            if positive {
                x -= 1;
                y += 1;
                descending = false;
            } else {
                x += 1;
            }
            shape = "east_west".into();
        }
        "ascending_north" => {
            if positive {
                z += 1;
            } else {
                z -= 1;
                y += 1;
                descending = false;
            }
            shape = "north_south".into();
        }
        "ascending_south" => {
            if positive {
                z += 1;
                y += 1;
                descending = false;
            } else {
                z -= 1;
            }
            shape = "north_south".into();
        }
        _ => {}
    }
    let power = rail_power(srv, dim, (x, y, z), positive, depth, &shape);
    if power > 0 {
        power
    } else if descending {
        rail_power(srv, dim, (x, y - 1, z), positive, depth, &shape)
    } else {
        0
    }
}

fn rail_power(srv: &dyn Server, dim: &str, pos: (i32, i32, i32), positive: bool, depth: i32, shape: &str) -> i32 {
    let Some(state) = srv.get_block_state(dim, to_ypos(pos)) else { return 0 };
    let (id, props) = split_state(&state);
    if id != "minecraft:powered_rail" {
        return 0;
    }
    let get = |k: &str| props.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| *v);
    let other = get("shape");
    let crosses = (shape == "east_west" && matches!(other, "north_south" | "ascending_north" | "ascending_south"))
        || (shape == "north_south" && matches!(other, "east_west" | "ascending_east" | "ascending_west"));
    if crosses || get("powered") != "true" {
        return 0;
    }
    if srv.redstone_power(dim, to_ypos(pos)).is_some_and(|p| p > 0) {
        8 - depth
    } else {
        rail_signal(srv, dim, pos, other, positive, depth + 1)
    }
}

/// `ScryingLensOverlayRegistry.getLines` for the block at `pos`.
pub fn lines(srv: &dyn Server, dim: &str, pos: (i32, i32, i32)) -> Vec<Line> {
    let Some(state) = srv.get_block_state(dim, to_ypos(pos)) else { return Vec::new() };
    let (id, props) = split_state(&state);
    let get = |k: &str| props.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| *v);
    let mut out = Vec::new();
    match id {
        Blocks::IMPETUS_RIGHTCLICK | Blocks::IMPETUS_LOOK | Blocks::IMPETUS_REDSTONE => {
            // `BlockEntityAbstractImpetus.applyScryingLensOverlay`.
            let imp = circle::impetus(srv, dim, pos);
            if imp.media < 0 {
                out.push(line(Items::AMETHYST_DUST, Text::key("hexcasting.debug.infinite_media")));
            } else {
                let dust = dust_amount(imp.media as f64 / DUST as f64);
                out.push(line(Items::AMETHYST_DUST, Text::key_args("hexcasting.tooltip.media", vec![Text::lit(dust)])));
            }
            if let Some((msg, item)) = &imp.display {
                out.push(line(item, msg.clone()));
            }
            if id == Blocks::IMPETUS_REDSTONE {
                match &imp.player {
                    Some((_, name)) => out.push(line(
                        "minecraft:player_head",
                        Text::key_args("hexcasting.tooltip.lens.impetus.redstone.bound", vec![Text::lit(name.clone())]),
                    )),
                    None => out.push(line("minecraft:barrier", Text::key("hexcasting.tooltip.lens.impetus.redstone.bound.none"))),
                }
            }
        }
        "minecraft:note_block" => {
            let note: i32 = get("note").parse().unwrap_or(0);
            let tone = |o: f32| ((note as f32 / 24.0 + o) * std::f32::consts::TAU).sin().mul_add(0.65, 0.35).max(0.0);
            let color = ((tone(0.0) * 255.0) as u32) << 16 | ((tone(1.0 / 3.0) * 255.0) as u32) << 8 | (tone(2.0 / 3.0) * 255.0) as u32;
            let instrument = get("instrument");
            let ordinal = INSTRUMENTS.iter().position(|i| *i == instrument).unwrap_or(0);
            out.push(colored("minecraft:music_disc_chirp", instrument_color(instrument), ordinal.to_string()));
            out.push(colored("minecraft:note_block", color, note.to_string()));
        }
        akashic::BOOKSHELF => {
            if let Some((_, iota)) = akashic::shelf(srv, dim, pos) {
                out.push(line("minecraft:book", iota.display()));
            }
        }
        "minecraft:comparator" => {
            let value = srv
                .get_block_nbt(dim, to_ypos(pos))
                .and_then(|n| crate::snbt::Nbt::parse(&n).get_i64("OutputSignal"))
                .unwrap_or(0) as i32;
            out.push(colored("minecraft:redstone", redstone_color(value, 15), value.to_string()));
            let compare = get("mode") == "compare";
            out.push(colored("minecraft:redstone_torch", redstone_color(if compare { 0 } else { 15 }, 15), if compare { ">=" } else { "-" }));
        }
        "minecraft:powered_rail" => {
            let power = powered_rail_strength(srv, dim, pos);
            out.push(colored("minecraft:powered_rail", redstone_color(power, 9), power.to_string()));
        }
        "minecraft:repeater" => {
            out.push(colored("minecraft:clock", 0xFFFF55, get("delay")));
        }
        _ => {}
    }
    // Any redstone source (but a comparator): what it gives off, first.
    if id != "minecraft:comparator"
        && let Some(signal) = srv.redstone_signal(dim, to_ypos(pos))
    {
        let s = signal as i32;
        out.insert(0, colored("minecraft:redstone", redstone_color(s, 15), s.to_string()));
    }
    out
}

// ── the server: who looks at what ───────────────────────────────────────────

/// What each player was last sent (an empty list is "nothing").
static SENT: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
static TICKS: Mutex<u64> = Mutex::new(0);

/// `SCRY_SIGHT`: a lens in either hand or on the head.
fn has_lens(srv: &dyn Server, player: &str) -> bool {
    let selected = srv.player_info(player).map_or(0, |p| p.selected_slot);
    [selected, SLOT_OFFHAND, HEAD_SLOT].into_iter().any(|slot| Stack::read(srv, player, slot).id == Items::LENS)
}

pub fn tick(srv: &dyn Server) {
    let now = {
        let mut t = TICKS.lock().unwrap();
        *t += 1;
        *t
    };
    if !now.is_multiple_of(EVERY) {
        return;
    }
    let mut sent = SENT.lock().unwrap();
    for name in srv.online_players() {
        let mut found = Vec::new();
        if has_lens(srv, &name)
            && let Some(p) = srv.player_info(&name)
            && let Some(e) = srv.entity_info(&p.uuid)
        {
            let eye = Vec3::new(e.pos.0, e.pos.1 + e.eye_height, e.pos.2);
            let end = eye + look_vec(srv, &p.uuid) * REACH;
            if let Some(hit) = srv.raycast_block(&e.dimension, eye.tuple(), end.tuple()) {
                found = lines(srv, &e.dimension, from_ypos(hit.pos));
            }
        }
        let bytes = encode(&found);
        match sent.iter_mut().find(|(n, _)| *n == name) {
            Some((_, last)) if *last == bytes => {}
            Some((_, last)) => {
                srv.send_to_player(&name, CH_LENS, &bytes);
                *last = bytes;
            }
            None => {
                srv.send_to_player(&name, CH_LENS, &bytes);
                sent.push((name, bytes));
            }
        }
    }
}

// ── the client: drawing them ────────────────────────────────────────────────

static SHOWN: Mutex<Vec<Line>> = Mutex::new(Vec::new());

/// The lines right of the crosshair, bottom-aligned on it: an item, then
/// its text wrapped to 40 % of the screen.
fn draw(gfx: &GfxContext) {
    let lines = SHOWN.lock().unwrap().clone();
    if lines.is_empty() {
        return;
    }
    let d = gfx.draw2d();
    let (w, h) = gfx.screen_size();
    let (w, h) = (w as f32, h as f32);
    let max_width = w / 2.0 * 0.8;
    const LINE: f32 = 9.0;
    let wrapped: Vec<(&Line, Vec<String>)> = lines
        .iter()
        .map(|l| {
            let mut rows: Vec<String> = Vec::new();
            for word in l.text.render().split(' ') {
                match rows.last_mut() {
                    Some(row) if d.text_width(&format!("{row} {word}"), "") <= max_width => {
                        row.push(' ');
                        row.push_str(word);
                    }
                    _ => rows.push(word.to_owned()),
                }
            }
            (l, rows)
        })
        .collect();
    let total: f32 = 8.0 + wrapped.iter().map(|(_, rows)| LINE + 6.0 + LINE * (rows.len().max(1) - 1) as f32).sum::<f32>();
    let x = w / 2.0 + 8.0;
    let mut y = h / 2.0 - total;
    for (l, rows) in wrapped {
        if !l.item.is_empty() {
            d.item(&l.item, x, y, 16.0);
        }
        let tx = if l.item.is_empty() { x } else { x + 18.0 };
        let color = 0xFF00_0000 | l.color.unwrap_or(0xFF_FFFF);
        for row in &rows {
            d.text(row, tx, y + 5.0, color, true);
            y += LINE;
        }
        if rows.is_empty() {
            y += LINE;
        }
        y += 6.0;
    }
}

pub fn register(registry: &mut Registry) {
    registry.on_tick(tick);
    registry.on_player_leave(|e, _, _| {
        SENT.lock().unwrap().retain(|(n, _)| *n != e.player_name);
        true
    });
    registry.on_client_packet(CH_LENS, |ev, _| {
        if let Some(lines) = decode(&ev.payload) {
            *SHOWN.lock().unwrap() = lines;
        }
    });
    registry.on_hud_render(draw);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dust_reads_as_the_original_formats_it() {
        assert_eq!(dust_amount(1234567.0), "1,234,567");
        assert_eq!(dust_amount(2.5), "2.5");
        assert_eq!(dust_amount(0.125), "0.13");
        assert_eq!(dust_amount(0.0), "0");
    }

    #[test]
    fn lines_survive_the_wire() {
        let ls = vec![colored("minecraft:redstone", 0xFF0000, "15"), line("minecraft:book", Text::key("x"))];
        assert_eq!(decode(&encode(&ls)), Some(ls));
    }

    #[test]
    fn dust_glows_with_its_power() {
        assert_eq!(redstone_color(0, 15), 0x4C0000);
        assert_eq!(redstone_color(15, 15) >> 16, 0xFF);
    }
}
