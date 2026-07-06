//! SpellList — functional lists of Iota.

use crate::iota::Iota;

#[derive(Debug, Clone, PartialEq)]
pub enum SpellList {
    Pair(Box<Iota>, Box<SpellList>),
    Array { idx: usize, data: Vec<Iota> },
    Nil,
}

impl SpellList {
    pub fn car(&self) -> Option<&Iota> {
        match self {
            SpellList::Pair(car, _) => Some(car),
            SpellList::Array { idx, data } => data.get(*idx),
            SpellList::Nil => None,
        }
    }

    pub fn cdr(&self) -> Option<SpellList> {
        match self {
            SpellList::Pair(_, cdr) => Some(*cdr.clone()),
            SpellList::Array { idx, data } => {
                if *idx + 1 < data.len() {
                    Some(SpellList::Array { idx: idx + 1, data: data.clone() })
                } else {
                    Some(SpellList::Nil)
                }
            }
            SpellList::Nil => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, SpellList::Nil)
    }

    pub fn len(&self) -> usize {
        match self {
            SpellList::Pair(_, cdr) => 1 + cdr.len(),
            SpellList::Array { idx, data } => data.len().saturating_sub(*idx),
            SpellList::Nil => 0,
        }
    }

    pub fn get_at(&self, start_idx: usize) -> Option<Iota> {
        self.to_vec().get(start_idx).cloned()
    }

    pub fn modify_at<F>(&self, start_idx: usize, f: F) -> SpellList
    where
        F: FnOnce(&SpellList) -> SpellList,
    {
        let mut vec = self.to_vec();
        if start_idx < vec.len() {
            let sub = SpellList::from_vec(vec.clone());
            let new_sub = f(&sub);
            vec.truncate(start_idx);
            Self::append_flattened(&mut vec, &new_sub);
        }
        SpellList::from_vec(vec)
    }

    fn append_flattened(vec: &mut Vec<Iota>, list: &SpellList) {
        match list {
            SpellList::Nil => {}
            SpellList::Pair(car, cdr) => {
                vec.push((**car).clone());
                Self::append_flattened(vec, cdr);
            }
            SpellList::Array { idx, data } => {
                vec.extend_from_slice(&data[*idx..]);
            }
        }
    }

    pub fn to_vec(&self) -> Vec<Iota> {
        let mut vec = Vec::new();
        Self::append_flattened(&mut vec, self);
        vec
    }

    pub fn iter(&self) -> IntoIter {
        self.clone().into_iter()
    }

    pub fn from_vec(vec: Vec<Iota>) -> Self {
        SpellList::Array { idx: 0, data: vec }
    }
}

impl Default for SpellList {
    fn default() -> Self {
        SpellList::Nil
    }
}

// Owned iterator — clones values as it goes.
pub struct IntoIter {
    list: Option<SpellList>,
}

impl Iterator for IntoIter {
    type Item = Iota;

    fn next(&mut self) -> Option<Self::Item> {
        match self.list.take()? {
            SpellList::Nil => None,
            SpellList::Pair(car, cdr) => {
                self.list = Some(*cdr);
                Some(*car)
            }
            SpellList::Array { mut idx, data } => {
                if idx < data.len() {
                    let item = data[idx].clone();
                    idx += 1;
                    self.list = Some(SpellList::Array { idx, data });
                    Some(item)
                } else {
                    None
                }
            }
        }
    }
}

impl IntoIterator for SpellList {
    type Item = Iota;
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter { list: Some(self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spell_list_basic() {
        let list = SpellList::from_vec(vec![
            Iota::Boolean(true),
            Iota::Double(3.14),
            Iota::Null,
        ]);
        assert_eq!(list.len(), 3);
        assert_eq!(list.car(), Some(&Iota::Boolean(true)));
    }

    #[test]
    fn test_modify_at() {
        let list = SpellList::from_vec(vec![
            Iota::Int(1),
            Iota::Int(2),
            Iota::Int(3),
        ]);
        let modified = list.modify_at(1, |_sub| {
            SpellList::from_vec(vec![Iota::Int(42)])
        });
        let vec = modified.to_vec();
        assert_eq!(vec, vec![Iota::Int(1), Iota::Int(42)]);
    }
}