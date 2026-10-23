//! The scrying lens (`ScryingLensOverlays`, `HexAdditionalRenderers
//! .tryRenderScryingLensOverlay`): held or worn, it shows what the block
//! looked at is up to — redstone, notes, an impetus' media and messages, a
//! bookshelf's iota.
//!
//! The server works the lines out (it has the impetuses and shelves) and
//! sends them to the player; the client draws them right of the crosshair.

use crate::casting::env::SLOT_HEAD;
use crate::res::Lang;
use yog_api::sync::Mutex;

use yog_api::ui::{Color, Vec2};
use yog_api::vanilla;
use yog_api::{GfxContext, Registry, split_state};

use crate::casting::env::{SLOT_OFFHAND, Stack, from_ypos, look_vec, to_ypos};
use crate::circle;
use crate::client::text::Text;
use crate::content::{Blocks, Items};
use crate::encoding::wire::{Reader, Writer};
use crate::item::media::DUST;
use crate::math::Vec3;
use crate::things::akashic;

/// Server → client: the lines to show (none: nothing).
pub const CH_LENS: &str = "hexcasting:lens";
/// Digits a thousands separator groups; hundredths a media amount shows.
const DIGIT_GROUP: usize = 3;
const CENTS: f64 = 100.0;
/// How far the lens sees, as a hand reaches.
const REACH: f64 = 5.0;
/// Every so many ticks the lines are worked out again.
const EVERY: u64 = 2;
/// The strongest redstone signal; a powered rail's (`PoweredRailBlock`:
/// the rails a signal runs along, and the strength by a powered one).
const REDSTONE_MAX: i32 = 15;
const RAIL_REACH: i32 = 8;
const RAIL_POWERED: i32 = 9;
/// `getColorForPower`: red's share of the power and its floor (lit, unlit);
/// green's and blue's shares and offsets.
const WIRE_RED: (f32, f32, f32) = (0.6, 0.4, 0.3);
const WIRE_GREEN: (f32, f32) = (0.7, 0.5);
const WIRE_BLUE: (f32, f32) = (0.6, 0.7);
/// A note block's notes (two octaves), and its color's rainbow: how much
/// of a tone swings, its floor.
const NOTES: f32 = 24.0;
const TONE: (f32, f32) = (0.65, 0.35);
/// Red, green and blue a third of the rainbow apart.
const TONE_PHASES: [f32; 3] = [0.0, 1.0 / 3.0, 2.0 / 3.0];
/// `ChatFormatting.YELLOW`, a repeater's delay.
const DELAY_COLOR: u32 = 0xFFFF55;

/// A line: the item drawn beside it, its color (`None`: white) and text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub item: String,
    pub color: Option<u32>,
    pub text: Text,
}

fn line(item: &str, text: Text) -> Line {
    Line {
        item: item.into(),
        color: None,
        text,
    }
}

fn colored(item: &str, color: u32, text: impl Into<String>) -> Line {
    Line {
        item: item.into(),
        color: Some(color),
        text: Text::lit(text),
    }
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
            Some(Line {
                item,
                color: (color >= 0).then_some(color as u32),
                text,
            })
        })
        .collect()
}

// ── what a block shows ──────────────────────────────────────────────────────

/// `RedStoneWireBlock.getColorForPower`.
fn redstone_color(power: i32, max: i32) -> u32 {
    let p = ((power * max) / REDSTONE_MAX).clamp(0, REDSTONE_MAX) as f32 / REDSTONE_MAX as f32;
    let (share, lit, unlit) = WIRE_RED;
    let r = p * share + if p > 0.0 { lit } else { unlit };
    let g = (p * p * WIRE_GREEN.0 - WIRE_GREEN.1).clamp(0.0, 1.0);
    let b = (p * p * WIRE_BLUE.0 - WIRE_BLUE.1).clamp(0.0, 1.0);
    rgb(r, g, b)
}

/// A color from 0..1 red, green, blue.
fn rgb(r: f32, g: f32, b: f32) -> u32 {
    let full = |v: f32| (v * f32::from(u8::MAX)) as u32;
    full(r) << 16 | full(g) << 8 | full(b)
}

const INSTRUMENTS: [&str; 23] = [
    "harp",
    "basedrum",
    "snare",
    "hat",
    "bass",
    "flute",
    "bell",
    "guitar",
    "chime",
    "xylophone",
    "iron_xylophone",
    "cow_bell",
    "didgeridoo",
    "bit",
    "banjo",
    "pling",
    "zombie",
    "skeleton",
    "creeper",
    "dragon",
    "wither_skeleton",
    "piglin",
    "custom_head",
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
    let rounded = (v * CENTS).round() / CENTS;
    let whole = rounded.trunc() as i64;
    let frac = ((rounded - whole as f64).abs() * CENTS).round() as i64;
    let digits = whole.abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(DIGIT_GROUP) {
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
fn powered_rail_strength(dim: &str, pos: (i32, i32, i32)) -> i32 {
    if yog_api::event::block::redstone_power(dim, to_ypos(pos)).is_some_and(|p| p > 0) {
        return RAIL_POWERED;
    }
    let shape = yog_api::event::block::property(dim, to_ypos(pos), "shape").unwrap_or_default();
    rail_signal(dim, pos, &shape, true, 0).max(rail_signal(dim, pos, &shape, false, 0))
}

fn rail_signal(dim: &str, pos: (i32, i32, i32), shape: &str, positive: bool, depth: i32) -> i32 {
    if depth >= RAIL_REACH {
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
    let power = rail_power(dim, (x, y, z), positive, depth, &shape);
    if power > 0 {
        power
    } else if descending {
        rail_power(dim, (x, y - 1, z), positive, depth, &shape)
    } else {
        0
    }
}

fn rail_power(dim: &str, pos: (i32, i32, i32), positive: bool, depth: i32, shape: &str) -> i32 {
    let Some(state) = yog_api::event::block::state(dim, to_ypos(pos)) else {
        return 0;
    };
    let (id, props) = split_state(&state);
    if id != vanilla::blocks::POWERED_RAIL {
        return 0;
    }
    let get = |k: &str| props.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| *v);
    let other = get("shape");
    let crosses = (shape == "east_west"
        && matches!(other, "north_south" | "ascending_north" | "ascending_south"))
        || (shape == "north_south"
            && matches!(other, "east_west" | "ascending_east" | "ascending_west"));
    if crosses || get("powered") != "true" {
        return 0;
    }
    if yog_api::event::block::redstone_power(dim, to_ypos(pos)).is_some_and(|p| p > 0) {
        RAIL_REACH - depth
    } else {
        rail_signal(dim, pos, other, positive, depth + 1)
    }
}

/// `ScryingLensOverlayRegistry.getLines` for the block at `pos`.
pub fn lines(dim: &str, pos: (i32, i32, i32)) -> Vec<Line> {
    let Some(state) = yog_api::event::block::state(dim, to_ypos(pos)) else {
        return Vec::new();
    };
    let (id, props) = split_state(&state);
    let get = |k: &str| props.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| *v);
    let mut out = match id {
        Blocks::IMPETUS_RIGHTCLICK | Blocks::IMPETUS_LOOK | Blocks::IMPETUS_REDSTONE => {
            impetus_lines(dim, pos, id == Blocks::IMPETUS_REDSTONE)
        }
        vanilla::blocks::NOTE_BLOCK => {
            note_lines(get("note").parse().unwrap_or(0), get("instrument"))
        }
        akashic::BOOKSHELF => akashic::shelf(dim, pos)
            .map(|(_, iota)| vec![line(vanilla::items::BOOK, iota.display())])
            .unwrap_or_default(),
        vanilla::items::COMPARATOR => comparator_lines(dim, pos, get("mode") == "compare"),
        vanilla::items::POWERED_RAIL => {
            let power = powered_rail_strength(dim, pos);
            vec![colored(
                vanilla::items::POWERED_RAIL,
                redstone_color(power, RAIL_POWERED),
                power.to_string(),
            )]
        }
        vanilla::items::REPEATER => vec![colored(vanilla::items::CLOCK, DELAY_COLOR, get("delay"))],
        _ => Vec::new(),
    };
    // Any redstone source (but a comparator): what it gives off, first.
    if id != vanilla::blocks::COMPARATOR
        && let Some(signal) = yog_api::event::block::redstone_signal(dim, to_ypos(pos))
    {
        let s = signal as i32;
        let given = colored(
            vanilla::items::REDSTONE,
            redstone_color(s, REDSTONE_MAX),
            s.to_string(),
        );
        out.insert(0, given);
    }
    out
}

/// `BlockEntityAbstractImpetus.applyScryingLensOverlay`: its media, what
/// it last said, and (a `redstone` one) whom it's bound to.
fn impetus_lines(dim: &str, pos: (i32, i32, i32), redstone: bool) -> Vec<Line> {
    let imp = circle::impetus(dim, pos);
    let media = if imp.media < 0 {
        Text::key(Lang::DEBUG_INFINITE_MEDIA_KEY)
    } else {
        let dust = dust_amount(imp.media as f64 / DUST as f64);
        Text::key_args(Lang::TOOLTIP_MEDIA_KEY, vec![Text::lit(dust)])
    };
    let mut out = vec![line(Items::AMETHYST_DUST, media)];
    if let Some((msg, item)) = &imp.display {
        out.push(line(item, msg.clone()));
    }
    if redstone {
        out.push(match &imp.player {
            Some((_, name)) => line(
                vanilla::items::PLAYER_HEAD,
                Text::key_args(
                    Lang::TOOLTIP_LENS_IMPETUS_REDSTONE_BOUND_KEY,
                    vec![Text::lit(name.clone())],
                ),
            ),
            None => line(
                vanilla::items::BARRIER,
                Text::key(Lang::TOOLTIP_LENS_IMPETUS_REDSTONE_BOUND_NONE_KEY),
            ),
        });
    }
    out
}

/// A note block's instrument (by its number, in its block's color) and
/// note (in the note particle's color).
fn note_lines(note: i32, instrument: &str) -> Vec<Line> {
    let tone = |o: f32| {
        ((note as f32 / NOTES + o) * std::f32::consts::TAU)
            .sin()
            .mul_add(TONE.0, TONE.1)
            .max(0.0)
    };
    let [r, g, b] = TONE_PHASES.map(tone);
    let color = rgb(r, g, b);
    let ordinal = INSTRUMENTS
        .iter()
        .position(|i| *i == instrument)
        .unwrap_or(0);
    vec![
        colored(
            vanilla::items::MUSIC_DISC_CHIRP,
            instrument_color(instrument),
            ordinal.to_string(),
        ),
        colored(vanilla::items::NOTE_BLOCK, color, note.to_string()),
    ]
}

/// A comparator's output, and whether it compares or subtracts.
fn comparator_lines(dim: &str, pos: (i32, i32, i32), compare: bool) -> Vec<Line> {
    let value = yog_api::event::block::nbt(dim, to_ypos(pos))
        .and_then(|n| crate::encoding::snbt::Nbt::parse(&n).get_i64("OutputSignal"))
        .unwrap_or(0) as i32;
    let mode = redstone_color(if compare { 0 } else { REDSTONE_MAX }, REDSTONE_MAX);
    vec![
        colored(
            vanilla::items::REDSTONE,
            redstone_color(value, REDSTONE_MAX),
            value.to_string(),
        ),
        colored(
            vanilla::items::REDSTONE_TORCH,
            mode,
            if compare { ">=" } else { "-" },
        ),
    ]
}

// ── the server: who looks at what ───────────────────────────────────────────

/// What each player was last sent (an empty list is "nothing").
static SENT: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
static TICKS: Mutex<u64> = Mutex::new(0);

/// `SCRY_SIGHT`: a lens in either hand or on the head.
fn has_lens(player: &str) -> bool {
    let selected = yog_api::event::player::info(player).map_or(0, |p| p.selected_slot);
    [selected, SLOT_OFFHAND, SLOT_HEAD]
        .into_iter()
        .any(|slot| Stack::read(player, slot).id == Items::LENS)
}

pub fn tick() {
    let now = {
        let mut t = TICKS.lock();
        *t += 1;
        *t
    };
    if !now.is_multiple_of(EVERY) {
        return;
    }
    let mut sent = SENT.lock();
    for name in yog_api::event::player::online() {
        let mut found = Vec::new();
        if has_lens(&name)
            && let Some(p) = yog_api::event::player::info(&name)
            && let Some(e) = yog_api::event::entity::info(&p.uuid)
        {
            let eye = Vec3::new(e.pos.x, e.pos.y + e.eye_height, e.pos.z);
            let end = eye + look_vec(&p.uuid) * REACH;
            if let Some(hit) =
                yog_api::event::world::raycast_block(e.dimension, eye.tuple(), end.tuple())
            {
                found = lines(&e.dimension, from_ypos(hit.pos));
            }
        }
        let bytes = encode(&found);
        match sent.iter_mut().find(|(n, _)| *n == name) {
            Some((_, last)) if *last == bytes => {}
            Some((_, last)) => {
                yog_api::event::net::send_to_player(&name, CH_LENS, &bytes);
                *last = bytes;
            }
            None => {
                yog_api::event::net::send_to_player(&name, CH_LENS, &bytes);
                sent.push((name, bytes));
            }
        }
    }
}

// ── the client: drawing them ────────────────────────────────────────────────

/// The lines' text wraps at this share of half the screen; a line's height,
/// the gap after each, from the crosshair; the item's size and gap, the
/// text's drop beside it; a line's color when it has none.
const WRAP_SHARE: f32 = 0.8;
const LINE: f32 = 9.0;
const GAP: f32 = 6.0;
const CROSSHAIR_GAP: f32 = 8.0;
const ITEM_SIZE: f32 = 16.0;
const ITEM_GAP: f32 = 2.0;
const TEXT_DOWN: f32 = 5.0;
const WHITE: u32 = 0xFF_FFFF;

static SHOWN: Mutex<Vec<Line>> = Mutex::new(Vec::new());

/// The lines right of the crosshair, bottom-aligned on it: an item, then
/// its text wrapped to 40 % of the screen.
fn draw(gfx: &GfxContext) {
    let lines = SHOWN.lock().clone();
    if lines.is_empty() {
        return;
    }
    let d = gfx.draw2d();
    let (w, h) = gfx.screen_size();
    let (w, h) = (w as f32, h as f32);
    let max_width = w / 2.0 * WRAP_SHARE;
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
    let total: f32 = CROSSHAIR_GAP
        + wrapped
            .iter()
            .map(|(_, rows)| LINE + GAP + LINE * (rows.len().max(1) - 1) as f32)
            .sum::<f32>();
    let x = w / 2.0 + CROSSHAIR_GAP;
    let mut y = h / 2.0 - total;
    for (l, rows) in wrapped {
        if !l.item.is_empty() {
            d.item(&l.item, Vec2::new(x, y), ITEM_SIZE);
        }
        let tx = if l.item.is_empty() {
            x
        } else {
            x + ITEM_SIZE + ITEM_GAP
        };
        let color = u32::from_be_bytes([u8::MAX, 0, 0, 0]) | l.color.unwrap_or(WHITE);
        for row in &rows {
            d.text(row, Vec2::new(tx, y + TEXT_DOWN), Color(color), true);
            y += LINE;
        }
        if rows.is_empty() {
            y += LINE;
        }
        y += GAP;
    }
}

pub fn register(registry: &mut Registry) {
    registry.on::<yog_api::event::server::game::Tick, _>(|_, _| tick());
    registry.on::<yog_api::event::player::session::Leave, _>(|e, _| {
        SENT.lock().retain(|(n, _)| *n != e.player_name);
        true
    });
    registry.on_client_packet(CH_LENS, |ev| {
        if let Some(lines) = decode(&ev.payload) {
            *SHOWN.lock() = lines;
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
        let ls = vec![
            colored("minecraft:redstone", 0xFF0000, "15"),
            line("minecraft:book", Text::key("x")),
        ];
        assert_eq!(decode(&encode(&ls)), Some(ls));
    }

    #[test]
    fn dust_glows_with_its_power() {
        assert_eq!(redstone_color(0, 15), 0x4C0000);
        assert_eq!(redstone_color(15, 15) >> 16, 0xFF);
    }
}
