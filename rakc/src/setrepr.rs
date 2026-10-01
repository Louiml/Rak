//! Insertion-ordered set storage, shared by both backends (spec 7A.11).
//!
//! # Why this is not a `HashSet`
//!
//! `docs/rak-features-spec.md` §7A.11 said sets should be "backed by the
//! existing `Map`", on the grounds that `Map` already iterates in insertion
//! order. That is not true of this codebase: both backends store maps in
//! `std::collections::HashMap` (`interpreter::Value::Map`,
//! `value::Value::Map`), whose iteration order is arbitrary and differs
//! between runs of the same program.
//!
//! Sets inherit that if they are built the way the spec suggests, and a set
//! that enumerates differently each run is not much use for deduplication,
//! diffing, or reproducible output. So a set here is an order-preserving
//! `Vec` of elements alongside a `HashSet` of their keys: insertion order for
//! iteration, O(1) average membership, and deterministic output.
//!
//! The spec's false premise about `Map` is called out in
//! `docs/V8-BACKEND-PARITY.md`; this module is the consequence of not acting on
//! it.
//!
//! # Element identity
//!
//! Sets need to decide when two elements are "the same". Rak's `PartialEq`
//! treats `0xA`, `10` and `10.0` as equal (cross-representation numeric
//! equality), which is the right rule for a set keyed on values: `set_of([1,
//! 0x1, 1.0])` has one element. So identity is `type discriminant + rendered
//! form`, which collapses the numeric representations while keeping `1` and
//! `"1"` distinct.

use std::collections::HashSet;
use std::sync::Arc;

/// A set of `V` that iterates in insertion order.
#[derive(Clone, Debug)]
pub struct SetRepr<V> {
    order: Vec<V>,
    index: HashSet<String>,
}

impl<V> SetRepr<V>
where
    V: Clone + SetElement,
{
    pub fn new() -> Self {
        SetRepr {
            order: Vec::new(),
            index: HashSet::new(),
        }
    }

    pub fn from_iter_ordered<I: IntoIterator<Item = V>>(items: I) -> Self {
        let mut s = SetRepr::new();
        for item in items {
            s.insert(item);
        }
        s
    }

    /// Add an element. Returns whether it was newly added, so `set_add` can
    /// report `true`/`false` for a genuine insert versus a duplicate.
    pub fn insert(&mut self, item: V) -> bool {
        let key = item.set_key();
        if self.index.insert(key) {
            self.order.push(item);
            true
        } else {
            false
        }
    }

    pub fn contains(&self, item: &V) -> bool {
        self.index.contains(&item.set_key())
    }

    pub fn remove(&mut self, item: &V) -> bool {
        let key = item.set_key();
        if self.index.remove(&key) {
            self.order.retain(|v| v.set_key() != key);
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Elements in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &V> {
        self.order.iter()
    }

    pub fn to_vec(&self) -> Vec<V> {
        self.order.clone()
    }

    pub fn union(&self, other: &SetRepr<V>) -> SetRepr<V> {
        let mut out = self.clone();
        for item in other.iter() {
            out.insert(item.clone());
        }
        out
    }

    /// Elements in both, in this set's insertion order.
    pub fn intersect(&self, other: &SetRepr<V>) -> SetRepr<V> {
        let mut out = SetRepr::new();
        for item in self.iter() {
            if other.contains(item) {
                out.insert(item.clone());
            }
        }
        out
    }

    /// Elements of this set that are not in `other`.
    pub fn diff(&self, other: &SetRepr<V>) -> SetRepr<V> {
        let mut out = SetRepr::new();
        for item in self.iter() {
            if !other.contains(item) {
                out.insert(item.clone());
            }
        }
        out
    }
}

impl<V> Default for SetRepr<V>
where
    V: Clone + SetElement,
{
    fn default() -> Self {
        SetRepr::new()
    }
}

/// A value that can be an element of a set.
///
/// Implemented once per backend value type rather than deriving an identity
/// generically, because what counts as "the same element" is a language
/// decision, not a container detail.
pub trait SetElement {
    fn set_key(&self) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum V {
        Int(i64),
        Str(String),
        Float(f64),
    }

    impl SetElement for V {
        fn set_key(&self) -> String {
            match self {
                V::Int(i) => format!("n:{}", i),
                V::Float(f) => format!("n:{}", *f as i64),
                V::Str(s) => format!("s:{}", s),
            }
        }
    }

    fn s(items: &[V]) -> SetRepr<V> {
        SetRepr::from_iter_ordered(items.iter().cloned())
    }

    #[test]
    fn preserves_insertion_order() {
        let set = s(&[V::Str("z".into()), V::Str("a".into()), V::Str("m".into())]);
        let order: Vec<String> = set.iter().map(|v| v.set_key()).collect();
        assert_eq!(order, vec!["s:z", "s:a", "s:m"]);
    }

    #[test]
    fn dedupes_and_reports_insertion() {
        let mut set = s(&[V::Int(1)]);
        assert!(set.insert(V::Int(1)), "re-adding reports false");
        assert_eq!(set.len(), 1);
        assert!(set.insert(V::Int(2)));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn int_and_float_are_the_same_element() {
        let mut set = s(&[V::Int(1)]);
        assert!(!set.insert(V::Float(1.0)), "1 and 1.0 collapse");
        assert!(set.contains(&V::Float(1.0)));
    }

    #[test]
    fn string_one_is_not_int_one() {
        let mut set = s(&[V::Int(1)]);
        assert!(set.insert(V::Str("1".into())));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn union_intersect_diff() {
        let a = s(&[V::Int(1), V::Int(2), V::Int(3)]);
        let b = s(&[V::Int(2), V::Int(3), V::Int(4)]);

        let u: Vec<i64> = a.union(&b).iter().filter_map(|v| match v {
            V::Int(i) => Some(*i),
            _ => None,
        }).collect();
        assert_eq!(u, vec![1, 2, 3, 4], "union keeps left order then appends");

        let i: Vec<i64> = a.intersect(&b).iter().filter_map(|v| match v {
            V::Int(i) => Some(*i),
            _ => None,
        }).collect();
        assert_eq!(i, vec![2, 3], "intersect keeps left order");

        let d: Vec<i64> = a.diff(&b).iter().filter_map(|v| match v {
            V::Int(i) => Some(*i),
            _ => None,
        }).collect();
        assert_eq!(d, vec![1], "diff keeps left order");
    }

    #[test]
    fn remove_keeps_order_of_the_rest() {
        let mut set = s(&[V::Int(1), V::Int(2), V::Int(3)]);
        assert!(set.remove(&V::Int(2)));
        assert!(!set.remove(&V::Int(99)));
        let left: Vec<i64> = set.iter().filter_map(|v| match v {
            V::Int(i) => Some(*i),
            _ => None,
        }).collect();
        assert_eq!(left, vec![1, 3]);
    }

    #[test]
    fn is_shareable_across_threads() {
        // Sets cross the thread boundary when a closure captures one, so the
        // representation has to be Send + Sync.
        fn assert_send_sync<T: Send + Sync>(_: &T) {}
        let set = Arc::new(s(&[V::Int(1)]));
        assert_send_sync(&set);
    }
}
