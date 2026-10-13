//! View model of the casting screen (`casting_screen.yui`): the server's
//! [`Report`]s turned into the stack panel, the status banner and the list
//! of patterns drawn this session. The grid itself is [`GridSurface`].

yog_api::ui_include!("ui/casting_screen");

use yog_api::Server;
use yog_kit::prelude::*;

use super::hex_grid::GridSurface;
use crate::casting::vm::angle_signature;
use crate::iota::Iota;
use crate::math::HexPattern;
use crate::protocol::{Action, Mishap, Outcome, Report, CH_RESET};
use crate::res::Strings;

const OK: u32 = 0xFF_4FD67A;
const MISHAP: u32 = 0xFF_E05A4F;
const PENDING: u32 = 0xFF_999A7B;

/// One row of the stack panel.
#[derive(Debug, Clone, PartialEq)]
pub struct IotaView {
    pub id: String,
    pub kind: String,
    pub value: String,
    pub accent: u32,
}

/// One drawn pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternView {
    pub id: String,
    pub name: String,
    pub signature: String,
    pub accent: u32,
    pending: bool,
}

#[derive(Default)]
pub struct CastingVm {
    /// Top first.
    stack: Vec<IotaView>,
    /// The last cast's message and colour.
    status: Option<(String, u32)>,
    /// Newest first.
    patterns: Vec<PatternView>,
    drawn: u32,
}

impl ViewModel for CastingVm {}

impl CastingVm {
    pub fn stack_rows(&self) -> impl Iterator<Item = &IotaView> + '_ {
        self.stack.iter()
    }
    pub fn patterns(&self) -> impl Iterator<Item = &PatternView> + '_ {
        self.patterns.iter()
    }
    pub fn has_status(&self) -> bool {
        self.status.is_some()
    }
    pub fn status_text(&self) -> String {
        self.status.as_ref().map(|(t, _)| t.clone()).unwrap_or_default()
    }
    pub fn status_color(&self) -> u32 {
        self.status.as_ref().map_or(OK, |(_, c)| *c)
    }

    /// The Clear button: asks the server to wipe the stack.
    pub fn clear(&mut self) {
        if let Some(srv) = yog_api::server() {
            srv.send_to_server(CH_RESET, b"");
        }
    }

    /// A pattern left the grid; it waits for the server's answer.
    pub fn drawn(&mut self, signature: String) {
        self.drawn += 1;
        self.patterns.insert(
            0,
            PatternView {
                id: self.drawn.to_string(),
                name: Strings::pattern_pending(),
                signature,
                accent: PENDING,
                pending: true,
            },
        );
    }

    /// The screen (re)opened: a new session.
    pub fn reset_session(&mut self) {
        self.patterns.clear();
        self.status = None;
    }

    /// The server's answer to the oldest pending pattern (or a reset).
    pub fn apply(&mut self, report: &Report) {
        let (name, status, color) = match &report.outcome {
            Outcome::Ok(Action::Reveal) => {
                let top = report.stack.first().map(iota_value).unwrap_or_default();
                (action_name(Action::Reveal), Strings::status_reveal(top), OK)
            }
            Outcome::Ok(a) => (action_name(*a), Strings::status_ok(action_name(*a)), OK),
            Outcome::Mishap(m) => (
                Strings::action_unknown(),
                Strings::status_mishap(mishap_text(m)),
                MISHAP,
            ),
            Outcome::Reset => (String::new(), Strings::status_reset(), OK),
        };
        if report.outcome != Outcome::Reset {
            if let Some(p) = self.patterns.iter_mut().rev().find(|p| p.pending) {
                p.pending = false;
                p.name = name;
                p.accent = color;
            }
        }
        self.status = Some((status, color));
        self.stack = report
            .stack
            .iter()
            .enumerate()
            .map(|(i, iota)| IotaView {
                id: i.to_string(),
                kind: iota_kind(iota),
                value: iota_value(iota),
                accent: iota_color(iota),
            })
            .collect();
    }
}

fn action_name(a: Action) -> String {
    match a {
        Action::MindsReflection => Strings::action_minds_reflection(),
        Action::CompassPurification => Strings::action_compass_purification(),
        Action::AlidadePurification => Strings::action_alidade_purification(),
        Action::Reveal => Strings::action_reveal(),
        Action::JestersGambit => Strings::action_jesters_gambit(),
        Action::ClearStack => Strings::action_clear_stack(),
    }
}

fn mishap_text(m: &Mishap) -> String {
    match m {
        Mishap::NoCaster => Strings::mishap_no_caster(),
        Mishap::EntityGone => Strings::mishap_entity_gone(),
        Mishap::ExpectedEntity => Strings::mishap_expected_entity(),
        Mishap::StackEmpty => Strings::mishap_stack_empty(),
        Mishap::NothingToReveal => Strings::mishap_nothing_to_reveal(),
        Mishap::NeedTwo => Strings::mishap_need_two(),
        Mishap::UnknownPattern(sig) => Strings::mishap_unknown_pattern(sig),
        Mishap::BadStrokes => Strings::mishap_bad_strokes(),
    }
}

fn iota_kind(i: &Iota) -> String {
    match i {
        Iota::Boolean(_) => Strings::iota_boolean(),
        Iota::Double(_) | Iota::Int(_) => Strings::iota_number(),
        Iota::String(_) => Strings::iota_string(),
        Iota::Entity(_) => Strings::iota_entity(),
        Iota::Vec3(..) => Strings::iota_vector(),
        Iota::Pattern(_) => Strings::iota_pattern(),
        Iota::List(_) => Strings::iota_list(),
        Iota::Null => Strings::iota_null(),
    }
}

fn iota_value(i: &Iota) -> String {
    match i {
        Iota::Boolean(true) => Strings::iota_true(),
        Iota::Boolean(false) => Strings::iota_false(),
        Iota::Double(d) => format!("{d:.2}"),
        Iota::Int(n) => n.to_string(),
        Iota::String(s) => format!("\"{s}\""),
        Iota::Entity(u) => u.to_string(),
        Iota::Vec3(x, y, z) => format!("({x:.2}, {y:.2}, {z:.2})"),
        Iota::Pattern(p) => angle_signature(p),
        Iota::List(l) => Strings::iota_list_value(l.len()),
        Iota::Null => Strings::iota_null_value(),
    }
}

fn iota_color(i: &Iota) -> u32 {
    match i {
        Iota::Boolean(_) => 0xFF_FFFF55,
        Iota::Double(_) | Iota::Int(_) => 0xFF_5555FF,
        Iota::Pattern(_) => 0xFF_FF55FF,
        Iota::Entity(_) => 0xFF_55FFFF,
        Iota::Vec3(..) => 0xFF_FF5555,
        Iota::List(_) => 0xFF_AAAAAA,
        Iota::String(_) => 0xFF_55FF55,
        Iota::Null => 0xFF_888888,
    }
}

/// The grid sent `p` to the server.
pub fn pattern_sent(p: &HexPattern) {
    let sig = angle_signature(p);
    if let Some(h) = handle() {
        h.update(move |vm| vm.drawn(sig));
    }
}

/// `hexcasting:open_grid`: a new session on a fresh grid.
pub fn open_session() {
    GridSurface::shared().clear();
    if let Some(h) = handle() {
        h.update(CastingVm::reset_session);
    }
    open();
}

/// `hexcasting:stack`: the server ran a pattern (or wiped the stack).
pub fn on_report(payload: &[u8]) {
    let Some(report) = Report::decode(payload) else {
        return;
    };
    match &report.outcome {
        Outcome::Reset => GridSurface::shared().clear(),
        o => GridSurface::shared().resolve(matches!(o, Outcome::Ok(_))),
    }
    if let Some(h) = handle() {
        h.update(move |vm| vm.apply(&report));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yog_kit::testing::TestHost;

    fn host() -> TestHost<View> {
        let mut h = TestHost::<View>::new(CastingVm::default());
        h.size(427.0, 240.0).open();
        h
    }

    #[test]
    fn reports_fill_the_panels() {
        let mut h = host();
        assert!(!h.is_visible(ids::STATUS));
        h.vm_mut().drawn("qaq".into());
        h.vm_mut().drawn("de".into());
        h.vm_mut().apply(&Report {
            outcome: Outcome::Ok(Action::MindsReflection),
            stack: vec![Iota::Vec3(1.0, 2.0, 3.0), Iota::Boolean(true)],
        });
        h.frame();
        assert!(h.is_visible(ids::STATUS));
        let vm = h.vm();
        assert_eq!(vm.stack.len(), 2);
        assert_eq!(vm.stack[0].value, "(1.00, 2.00, 3.00)");
        assert_eq!(vm.stack[0].accent, 0xFF_FF5555);
        // The oldest pending pattern got the answer.
        assert_eq!(vm.patterns[1].name, Strings::action_minds_reflection());
        assert!(vm.patterns[0].pending);
        assert_eq!(vm.status_color(), OK);

        h.vm_mut().apply(&Report {
            outcome: Outcome::Mishap(Mishap::NothingToReveal),
            stack: vec![],
        });
        assert_eq!(h.vm().patterns[0].accent, MISHAP);
        assert_eq!(
            h.vm().status_text(),
            Strings::status_mishap(Strings::mishap_nothing_to_reveal())
        );
        assert!(h.vm().stack.is_empty());
    }
}
