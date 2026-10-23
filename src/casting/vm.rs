//! The casting VM (Hex Casting's `CastingVM`, `CastResult`,
//! `OperatorSideEffect` and `HexEvalSounds`).

use std::sync::Arc;

use crate::casting::actions::{self, Lookup};
use crate::casting::cont::{Cont, Frame};
use crate::casting::env::{CastEnv, MAX_OP_COUNT, Spray};
use crate::casting::image::Image;
use crate::casting::mishap::{Context, Mishap};
use crate::iota::Iota;
use crate::math::{HexDir, HexPattern};
use crate::protocol::ResolvedType;
use crate::text::Text;

/// `HexEvalSounds`, in priority order (a cast keeps the loudest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Sound {
    #[default]
    Nothing,
    Normal,
    Spell,
    Hermes,
    Thoth,
    /// A spell without a casting sound (Beep, Flay Mind).
    Mute,
    Mishap,
}

impl Sound {
    pub fn id(self) -> Option<&'static str> {
        Some(match self {
            Sound::Nothing | Sound::Mute => return None,
            Sound::Normal => crate::res::SoundEvents::CASTING_CAST_NORMAL,
            Sound::Spell => crate::res::SoundEvents::CASTING_CAST_SPELL,
            Sound::Hermes => crate::res::SoundEvents::CASTING_CAST_HERMES,
            Sound::Thoth => crate::res::SoundEvents::CASTING_CAST_THOTH,
            Sound::Mishap => crate::res::SoundEvents::CASTING_CAST_FAIL,
        })
    }
}

/// What a spell does when it goes off (`RenderedSpell.cast`).
pub type SpellFn = Box<dyn FnOnce(&mut CastEnv, &mut Image)>;

/// `OperatorSideEffect`: what happens after an action, in order.
pub enum SideEffect {
    Mishap(Mishap, Context),
    ConsumeMedia(i64),
    AttemptSpell(SpellFn),
    Particles(Spray),
}

/// `SpellAction.Result`.
pub struct Spell {
    pub cost: i64,
    pub particles: Vec<Spray>,
    pub effect: SpellFn,
    pub ops: i64,
    /// `hasCastingSound`.
    pub sound: bool,
}

impl Spell {
    pub fn new(
        cost: i64,
        particles: Vec<Spray>,
        effect: impl FnOnce(&mut CastEnv, &mut Image) + 'static,
    ) -> Self {
        Spell {
            cost,
            particles,
            effect: Box::new(effect),
            ops: 1,
            sound: true,
        }
    }

    pub fn mute(mut self) -> Self {
        self.sound = false;
        self
    }
}

/// `OperationResult`.
pub struct OpResult {
    pub image: Image,
    pub effects: Vec<SideEffect>,
    pub cont: Cont,
    pub sound: Sound,
}

impl OpResult {
    /// No side effects, the normal sound.
    pub fn new(image: Image, cont: &Cont) -> Self {
        OpResult {
            image,
            effects: Vec::new(),
            cont: cont.clone(),
            sound: Sound::Normal,
        }
    }

    pub fn with_sound(mut self, sound: Sound) -> Self {
        self.sound = sound;
        self
    }
}

/// `CastResult`.
pub struct CastResult {
    pub cont: Cont,
    pub image: Option<Image>,
    pub effects: Vec<SideEffect>,
    pub kind: ResolvedType,
    pub sound: Sound,
}

impl CastResult {
    fn mishap(cont: Cont, image: Option<Image>, mishap: Mishap, ctx: Context) -> Self {
        let kind = mishap.resolution_type();
        CastResult {
            cont,
            image,
            effects: vec![SideEffect::Mishap(mishap, ctx)],
            kind,
            sound: Sound::Mishap,
        }
    }
}

/// `ExecutionClientView`: what the caster's client is told.
#[derive(Debug, Clone)]
pub struct ClientView {
    pub stack_clear: bool,
    pub kind: ResolvedType,
    pub stack: Vec<Iota>,
    pub ravenmind: Option<Iota>,
}

pub struct Vm {
    pub image: Image,
    pub env: CastEnv,
}

impl Vm {
    pub fn new(image: Image, env: CastEnv) -> Self {
        Vm { image, env }
    }

    /// `queueExecuteAndWrapIotas`: runs `iotas` to the end (or the first
    /// failure).
    pub fn queue_execute(&mut self, iotas: Vec<Iota>) -> ClientView {
        let mut cont = Cont::done().push(Frame::Evaluate {
            list: Arc::new(iotas),
            idx: 0,
            metacasting: false,
        });
        let mut last = ResolvedType::Unresolved;
        let mut early_exit = false;
        while !early_exit {
            let Some(frame) = cont.frame() else { break };
            let mut result = self.evaluate(frame, cont.next());
            if let Some(img) = &result.image {
                let over = if Iota::too_large(&img.stack) {
                    Some(Mishap::StackSize)
                } else if img.ops_consumed > MAX_OP_COUNT {
                    Some(Mishap::EvalTooMuch)
                } else {
                    None
                };
                if let Some(m) = over {
                    result.image = None;
                    result.effects = vec![SideEffect::Mishap(m, Context::default())];
                    result.kind = ResolvedType::Errored;
                    result.sound = Sound::Mishap;
                }
            }
            if let Some(img) = result.image.take() {
                self.image = img;
            }
            self.post_execution(&result);
            cont = result.cont;
            last = result.kind;
            self.perform(result.effects);
            early_exit = !last.success();
        }
        if !cont.is_done() {
            last = if last.success() {
                ResolvedType::Evaluated
            } else {
                ResolvedType::Errored
            };
        }
        let img = &self.image;
        ClientView {
            stack_clear: img.stack.is_empty()
                && img.paren_count == 0
                && !img.escape_next
                && img.ravenmind.is_none(),
            kind: last,
            stack: img.stack.clone(),
            ravenmind: img.ravenmind.clone(),
        }
    }

    /// `postExecution`: the sound, and the mishap messages.
    fn post_execution(&mut self, result: &CastResult) {
        self.env.post_execution(result.sound);
        for e in &result.effects {
            if let SideEffect::Mishap(m, ctx) = e
                && let Some(msg) = m.message_with_name(&self.env, ctx)
            {
                self.env.report_mishap(&msg);
            }
        }
    }

    fn perform(&mut self, effects: Vec<SideEffect>) {
        for e in effects {
            match e {
                SideEffect::Mishap(m, ctx) => {
                    let spray = m.particle_spray(&self.env);
                    self.env.mishap_spray(&spray, m.accent_color());
                    m.execute(&mut self.env, &ctx, &mut self.image.stack);
                }
                SideEffect::ConsumeMedia(n) => {
                    self.env.extract_media(n, false);
                }
                SideEffect::AttemptSpell(f) => f(&mut self.env, &mut self.image),
                SideEffect::Particles(s) => self.env.spray(&s),
            }
        }
    }

    /// `ContinuationFrame.evaluate`; `cont` is what's under `frame`.
    fn evaluate(&mut self, frame: &Frame, cont: Cont) -> CastResult {
        match frame {
            Frame::Evaluate {
                list,
                idx,
                metacasting,
            } => {
                let Some(iota) = list.get(*idx) else {
                    return CastResult {
                        cont,
                        image: None,
                        effects: Vec::new(),
                        kind: ResolvedType::Evaluated,
                        sound: Sound::Hermes,
                    };
                };
                let new_cont = if idx + 1 < list.len() {
                    cont.push(Frame::Evaluate {
                        list: list.clone(),
                        idx: idx + 1,
                        metacasting: *metacasting,
                    })
                } else {
                    cont
                };
                let mut update = self.execute_inner(iota, new_cont);
                if *metacasting && update.sound != Sound::Mishap {
                    update.sound = Sound::Hermes;
                }
                update
            }
            Frame::ForEach {
                data,
                data_idx,
                code,
                base,
                acc,
            } => {
                let (mut stack, new_acc) = match base {
                    None => (self.image.stack.clone(), acc.clone()),
                    Some(b) => {
                        let mut a = acc.clone();
                        a.extend(self.image.stack.iter().cloned());
                        (b.clone(), a)
                    }
                };
                let (top, image, cont) = match data.get(*data_idx) {
                    Some(item) => {
                        let cont = cont
                            .push(Frame::ForEach {
                                data: data.clone(),
                                data_idx: data_idx + 1,
                                code: code.clone(),
                                base: Some(stack.clone()),
                                acc: new_acc,
                            })
                            .push(Frame::Evaluate {
                                list: code.clone(),
                                idx: 0,
                                metacasting: true,
                            });
                        (item.clone(), self.image.clone().with_used_op(), cont)
                    }
                    None => (Iota::List(new_acc), self.image.clone(), cont),
                };
                stack.push(top);
                let mut image = image.with_reset_escape();
                image.stack = stack;
                CastResult {
                    cont,
                    image: Some(image),
                    effects: Vec::new(),
                    kind: ResolvedType::Evaluated,
                    sound: Sound::Thoth,
                }
            }
            Frame::FinishEval => CastResult {
                cont,
                image: None,
                effects: Vec::new(),
                kind: ResolvedType::Evaluated,
                sound: Sound::Nothing,
            },
        }
    }

    /// `executeInner`.
    fn execute_inner(&mut self, iota: &Iota, cont: Cont) -> CastResult {
        if self.image.escape_next {
            let mut img = self.image.clone();
            img.escape_next = false;
            if img.paren_count > 0 {
                img = img.with_new_parenthesized(iota.clone(), true);
            } else {
                img.stack.push(iota.clone());
            }
            return CastResult {
                cont,
                image: Some(img),
                effects: Vec::new(),
                kind: ResolvedType::Escaped,
                sound: Sound::Normal,
            };
        }
        let in_parens = self.image.paren_count > 0;
        match iota {
            Iota::Pattern(p) => self.lookup_and_operate(p, cont, in_parens),
            _ if in_parens => CastResult {
                cont,
                image: Some(
                    self.image
                        .clone()
                        .with_new_parenthesized(iota.clone(), false),
                ),
                effects: Vec::new(),
                kind: ResolvedType::Escaped,
                sound: Sound::Normal,
            },
            Iota::Continuation(c) => CastResult {
                cont: c.clone(),
                image: Some(self.image.clone()),
                effects: Vec::new(),
                kind: ResolvedType::Evaluated,
                sound: Sound::Hermes,
            },
            other => CastResult {
                cont,
                image: None,
                effects: vec![SideEffect::Mishap(
                    Mishap::Unescaped(other.clone()),
                    Context {
                        pattern: Some(HexPattern::new(HexDir::West, Vec::new())),
                        name: None,
                    },
                )],
                kind: ResolvedType::Invalid,
                sound: Sound::Mishap,
            },
        }
    }

    /// `PatternIota.lookupAndOperate`.
    fn lookup_and_operate(
        &mut self,
        pattern: &HexPattern,
        cont: Cont,
        in_parens: bool,
    ) -> CastResult {
        match self.operate(pattern, &cont, in_parens) {
            Ok(r) => r,
            Err((mishap, name)) => {
                let wipe = matches!(
                    cont.frame(),
                    Some(Frame::Evaluate {
                        metacasting: true,
                        ..
                    })
                );
                let image = wipe.then(|| self.image.clone().with_reset_escape());
                let ctx = Context {
                    pattern: Some(pattern.clone()),
                    name,
                };
                CastResult::mishap(cont, image, mishap, ctx)
            }
        }
    }

    /// Fails with the mishap and the name of what was drawn (made only
    /// then: most patterns never need it).
    fn operate(
        &mut self,
        pattern: &HexPattern,
        cont: &Cont,
        in_parens: bool,
    ) -> Result<CastResult, (Mishap, Option<Text>)> {
        let image = self.image.clone();
        let (result, kind) = match actions::lookup(pattern) {
            Lookup::Action(action) => {
                let named = |m| (m, Some(action.name()));
                // REQUIRES_ENLIGHTENMENT_IN_PARENS is empty.
                if !in_parens && action.great() && !self.env.is_enlightened() {
                    return Err(named(Mishap::Unenlightened));
                }
                if in_parens {
                    action
                        .operate_in_parens(
                            &mut self.env,
                            image,
                            cont,
                            &Iota::Pattern(pattern.clone()),
                        )
                        .map_err(named)?
                } else {
                    let r = (action.op)(&mut self.env, image, cont).map_err(named)?;
                    (r, ResolvedType::Evaluated)
                }
            }
            Lookup::Special(special) => {
                if in_parens {
                    (
                        OpResult::new(
                            image.with_new_parenthesized(Iota::Pattern(pattern.clone()), false),
                            cont,
                        ),
                        ResolvedType::Escaped,
                    )
                } else {
                    let r = special
                        .operate(&mut self.env, image, cont)
                        .map_err(|m| (m, Some(special.name())))?;
                    (r, ResolvedType::Evaluated)
                }
            }
            Lookup::Nothing if in_parens => (
                OpResult::new(
                    image.with_new_parenthesized(Iota::Pattern(pattern.clone()), false),
                    cont,
                ),
                ResolvedType::Escaped,
            ),
            Lookup::Nothing => return Err((Mishap::InvalidPattern(Some(pattern.clone())), None)),
        };
        Ok(CastResult {
            cont: result.cont,
            image: Some(result.image),
            effects: result.effects,
            kind,
            sound: result.sound,
        })
    }
}
