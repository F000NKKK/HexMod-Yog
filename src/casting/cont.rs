//! Continuations — what the VM still has to do (Hex Casting's
//! `SpellContinuation` and its frames). A persistent linked list, so Iris'
//! Gambit can put "the rest of the hex" on the stack as an iota.

use std::sync::Arc;

use crate::iota::{Iota, read_stack, write_stack};
use crate::wire::{Reader, Writer};

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// Run `list[idx..]` (Hermes' Gambit, a drawn pattern, a hex).
    Evaluate { list: Arc<Vec<Iota>>, idx: usize, metacasting: bool },
    /// Thoth's Gambit: run `code` once per remaining `data` item.
    ForEach {
        data: Arc<Vec<Iota>>,
        data_idx: usize,
        code: Arc<Vec<Iota>>,
        /// The stack under the loop (`None` before the first iteration).
        base: Option<Vec<Iota>>,
        acc: Vec<Iota>,
    },
    /// The boundary Hermes' Gambit installs, where Charon's Gambit stops.
    FinishEval,
}

impl Frame {
    /// Charon's Gambit unwinding through this frame: whether to stop here,
    /// and the stack after it.
    pub fn break_downwards(&self, stack: Vec<Iota>) -> (bool, Vec<Iota>) {
        match self {
            Frame::Evaluate { .. } => (false, stack),
            Frame::ForEach { base, acc, .. } => {
                let mut out = base.clone().unwrap_or_default();
                let mut all = acc.clone();
                all.extend(stack);
                out.push(Iota::List(all));
                (true, out)
            }
            Frame::FinishEval => (true, stack),
        }
    }

    fn size(&self) -> usize {
        match self {
            Frame::Evaluate { list, idx, .. } => list.len().saturating_sub(*idx),
            Frame::ForEach { data, data_idx, code, base, acc } => {
                data.len().saturating_sub(*data_idx)
                    + code.len()
                    + acc.len()
                    + base.as_ref().map_or(0, Vec::len)
            }
            Frame::FinishEval => 0,
        }
    }

    fn write(&self, w: &mut Writer) {
        match self {
            Frame::Evaluate { list, idx, metacasting } => {
                w.tag('E').int(*metacasting as i64);
                write_stack(w, &list[(*idx).min(list.len())..]);
            }
            Frame::ForEach { data, data_idx, code, base, acc } => {
                w.tag('F');
                write_stack(w, &data[(*data_idx).min(data.len())..]);
                write_stack(w, code);
                match base {
                    Some(b) => {
                        w.int(1);
                        write_stack(w, b);
                    }
                    None => {
                        w.int(0);
                    }
                }
                write_stack(w, acc);
            }
            Frame::FinishEval => {
                w.tag('X');
            }
        }
    }

    fn read(r: &mut Reader) -> Option<Self> {
        Some(match r.tag()? {
            'E' => {
                let metacasting = r.int()? != 0;
                Frame::Evaluate { list: Arc::new(read_stack(r)?), idx: 0, metacasting }
            }
            'F' => {
                let data = Arc::new(read_stack(r)?);
                let code = Arc::new(read_stack(r)?);
                let base = if r.int()? != 0 { Some(read_stack(r)?) } else { None };
                let acc = read_stack(r)?;
                Frame::ForEach { data, data_idx: 0, code, base, acc }
            }
            'X' => Frame::FinishEval,
            _ => return None,
        })
    }
}

#[derive(Debug, PartialEq)]
struct Node {
    frame: Frame,
    next: Cont,
}

/// A stack of frames; `Cont::default()` is "done".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cont(Option<Arc<Node>>);

impl Cont {
    pub fn done() -> Self {
        Cont(None)
    }

    pub fn is_done(&self) -> bool {
        self.0.is_none()
    }

    pub fn push(&self, frame: Frame) -> Cont {
        Cont(Some(Arc::new(Node { frame, next: self.clone() })))
    }

    /// The top frame.
    pub fn frame(&self) -> Option<&Frame> {
        self.0.as_ref().map(|n| &n.frame)
    }

    /// The continuation under the top frame.
    pub fn next(&self) -> Cont {
        self.0.as_ref().map(|n| n.next.clone()).unwrap_or_default()
    }

    /// Total iotas held by the frames (for the size limit).
    pub fn size(&self) -> usize {
        let mut n = 0;
        let mut at = self;
        while let Some(node) = &at.0 {
            n += node.frame.size();
            at = &node.next;
        }
        n
    }

    pub fn write(&self, w: &mut Writer) {
        let mut frames = Vec::new();
        let mut at = self;
        while let Some(node) = &at.0 {
            frames.push(&node.frame);
            at = &node.next;
        }
        w.int(frames.len() as i64);
        for f in frames {
            f.write(w);
        }
    }

    pub fn read(r: &mut Reader) -> Option<Self> {
        let n = r.int()?;
        let mut frames = Vec::new();
        for _ in 0..n {
            frames.push(Frame::read(r)?);
        }
        let mut cont = Cont::done();
        for f in frames.into_iter().rev() {
            cont = cont.push(f);
        }
        Some(cont)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuations_round_trip() {
        let c = Cont::done()
            .push(Frame::FinishEval)
            .push(Frame::ForEach {
                data: Arc::new(vec![Iota::Double(1.0), Iota::Double(2.0)]),
                data_idx: 1,
                code: Arc::new(vec![Iota::Null]),
                base: Some(vec![Iota::Bool(true)]),
                acc: vec![],
            })
            .push(Frame::Evaluate {
                list: Arc::new(vec![Iota::Null, Iota::Garbage]),
                idx: 0,
                metacasting: true,
            });
        let i = Iota::Continuation(c.clone());
        let back = Iota::deserialize(&i.serialize());
        // ForEach's consumed data is dropped on the way out.
        let Iota::Continuation(b) = back else { panic!() };
        assert_eq!(b.size(), c.size());
        assert!(matches!(b.next().frame(), Some(Frame::ForEach { data, .. }) if data.len() == 1));
    }
}
