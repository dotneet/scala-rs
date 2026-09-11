//! The iteration order of the Scala 2.13 collections nsc's pattern-match
//! analysis runs on, where that order decides what nsc reports.
//!
//! nsc's DPLL solver (`Solving.scala`) keeps a clause as an immutable
//! `Set[Lit]` and branches on `clause.head`; `MatchOptimization` reports the
//! duplicated alternatives of a `groupBy` in the order of an immutable
//! `HashMap`. Neither order is an accident of the input: it is the order of
//! the library's `Set1`..`Set4` (insertion order) and of its CHAMP hash trie
//! (`HashSet` / `HashMap`, ordered by the bits of the improved hash), and a
//! faithful port has to reproduce both to report the same counter-examples.

/// `scala.collection.Hashing.improve`.
pub(crate) fn improve(hcode: i32) -> i32 {
    let mut h: i32 = hcode.wrapping_add(!(hcode.wrapping_shl(9)));
    h ^= ((h as u32) >> 14) as i32;
    h = h.wrapping_add(h.wrapping_shl(4));
    h ^ (((h as u32) >> 10) as i32)
}

/// The iteration order of a CHAMP trie (`immutable.HashSet`/`HashMap`)
/// holding `items`, each with its `##`.
///
/// A node stores the keys whose 5-bit hash fragment at its level is unique
/// as *payload*, ordered by fragment, and the rest in sub-nodes, ordered by
/// fragment as well; the iterator yields a node's payload before descending
/// into its sub-nodes (`ChampBaseIterator`). Keys with the same full hash
/// would share a collision node in insertion order.
pub(crate) fn champ_order<T: Clone>(items: &[(T, i32)]) -> Vec<T> {
    let improved: Vec<(T, u32)> = items
        .iter()
        .map(|(t, h)| (t.clone(), improve(*h) as u32))
        .collect();
    let mut out = Vec::with_capacity(items.len());
    champ_level(&improved, 0, &mut out);
    out
}

fn champ_level<T: Clone>(items: &[(T, u32)], shift: u32, out: &mut Vec<T>) {
    if shift >= 32 {
        // Full-hash collision: insertion order.
        out.extend(items.iter().map(|(t, _)| t.clone()));
        return;
    }
    let mut buckets: std::collections::BTreeMap<u32, Vec<(T, u32)>> = Default::default();
    for (t, h) in items {
        buckets
            .entry((h >> shift) & 31)
            .or_default()
            .push((t.clone(), *h));
    }
    // Payload first (fragments with one key), then the sub-nodes.
    for group in buckets.values() {
        if group.len() == 1 {
            out.push(group[0].0.clone());
        }
    }
    for group in buckets.values() {
        if group.len() > 1 {
            champ_level(group, shift + 5, out);
        }
    }
}

/// `scala.util.hashing.MurmurHash3.mix`.
pub(crate) fn murmur_mix(hash: i32, data: i32) -> i32 {
    let h = murmur_mix_last(hash, data);
    h.rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64u32 as i32)
}

/// `MurmurHash3.mixLast`.
pub(crate) fn murmur_mix_last(hash: i32, data: i32) -> i32 {
    let mut k = data;
    k = k.wrapping_mul(0xcc9e2d51u32 as i32);
    k = k.rotate_left(15);
    k = k.wrapping_mul(0x1b873593);
    hash ^ k
}

/// `MurmurHash3.finalizeHash`.
pub(crate) fn murmur_finalize(hash: i32, length: i32) -> i32 {
    let mut h = hash ^ length;
    h ^= ((h as u32) >> 16) as i32;
    h = h.wrapping_mul(0x85ebca6bu32 as i32);
    h ^= ((h as u32) >> 13) as i32;
    h = h.wrapping_mul(0xc2b2ae35u32 as i32);
    h ^= ((h as u32) >> 16) as i32;
    h
}

/// `java.lang.String.hashCode`.
pub(crate) fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}

/// An immutable Scala `Set` as far as its order goes: `Set1`..`Set4` keep
/// insertion order; a fifth element turns it into a `HashSet`, which keeps
/// the trie order from then on (removing elements does not turn it back).
/// `ListSet` keeps insertion order at any size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScalaSet<T> {
    Small(Vec<T>),
    Hash(Vec<T>),
    List(Vec<T>),
}

pub(crate) trait ScalaHash {
    /// The element's `##`.
    fn scala_hash(&self) -> i32;
}

impl<T: Clone + PartialEq + ScalaHash> ScalaSet<T> {
    pub(crate) fn empty() -> Self {
        ScalaSet::Small(Vec::new())
    }

    pub(crate) fn empty_list() -> Self {
        ScalaSet::List(Vec::new())
    }

    /// `Set.from(xs)` / `xs.toSet`: added one by one.
    pub(crate) fn from_iter(xs: impl IntoIterator<Item = T>) -> Self {
        let mut s = Self::empty();
        for x in xs {
            s = s.incl(x);
        }
        s
    }

    /// `xs.to(ListSet)`.
    pub(crate) fn list_from(xs: impl IntoIterator<Item = T>) -> Self {
        let mut s = Self::empty_list();
        for x in xs {
            s = s.incl(x);
        }
        s
    }

    pub(crate) fn elems(&self) -> &[T] {
        match self {
            ScalaSet::Small(v) | ScalaSet::Hash(v) | ScalaSet::List(v) => v,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.elems().len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.elems().is_empty()
    }

    pub(crate) fn contains(&self, x: &T) -> bool {
        self.elems().contains(x)
    }

    pub(crate) fn head(&self) -> Option<&T> {
        self.elems().first()
    }

    fn rehash(v: Vec<T>) -> Vec<T> {
        let items: Vec<(T, i32)> = v.into_iter().map(|x| {
            let h = x.scala_hash();
            (x, h)
        }).collect();
        champ_order(&items)
    }

    pub(crate) fn incl(self, x: T) -> Self {
        if self.contains(&x) {
            return self;
        }
        match self {
            ScalaSet::Small(mut v) => {
                if v.len() < 4 {
                    v.push(x);
                    ScalaSet::Small(v)
                } else {
                    v.push(x);
                    ScalaSet::Hash(Self::rehash(v))
                }
            }
            ScalaSet::Hash(mut v) => {
                v.push(x);
                ScalaSet::Hash(Self::rehash(v))
            }
            ScalaSet::List(mut v) => {
                v.push(x);
                ScalaSet::List(v)
            }
        }
    }

    pub(crate) fn excl(self, x: &T) -> Self {
        match self {
            ScalaSet::Small(mut v) => {
                v.retain(|e| e != x);
                ScalaSet::Small(v)
            }
            ScalaSet::Hash(mut v) => {
                v.retain(|e| e != x);
                ScalaSet::Hash(v)
            }
            ScalaSet::List(mut v) => {
                v.retain(|e| e != x);
                ScalaSet::List(v)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ScalaHash for i32 {
        fn scala_hash(&self) -> i32 {
            *self
        }
    }

    #[test]
    fn small_sets_keep_insertion_order() {
        let s = ScalaSet::from_iter([3, -1, 7, 2]);
        assert_eq!(s.elems(), &[3, -1, 7, 2]);
        let s = s.excl(&-1);
        assert_eq!(s.elems(), &[3, 7, 2]);
    }

    fn order(xs: &[i32]) -> Vec<i32> {
        ScalaSet::from_iter(xs.iter().copied()).elems().to_vec()
    }

    fn remove2(xs: &[i32]) -> Vec<i32> {
        ScalaSet::from_iter(xs.iter().copied())
            .excl(&xs[0])
            .excl(&xs[1])
            .elems()
            .to_vec()
    }

    /// Orders printed by scala 2.13.16 for `Set.empty[Lit] ++ xs` (with
    /// `Lit(v).hashCode == v`) and for the same set minus its first two.
    #[test]
    fn hash_sets_iterate_in_scala_order() {
        assert_eq!(order(&[1, 2, 3, 4, 5]), [5, 1, 2, 3, 4]);
        assert_eq!(remove2(&[1, 2, 3, 4, 5]), [5, 3, 4]);
        assert_eq!(order(&[5, 4, 3, 2, 1]), [5, 1, 2, 3, 4]);
        assert_eq!(remove2(&[5, 4, 3, 2, 1]), [1, 2, 3]);
        assert_eq!(order(&[-1, -2, 3, 7, 12, 40]), [12, 7, 3, -1, 40, -2]);
        assert_eq!(remove2(&[-1, -2, 3, 7, 12, 40]), [12, 7, 3, 40]);
        assert_eq!(order(&[1, 33, 65, 2, 34]), [1, 33, 65, 2, 34]);
        assert_eq!(remove2(&[1, 33, 65, 2, 34]), [65, 2, 34]);
        let one_to_20: Vec<i32> = (1..=20).collect();
        assert_eq!(
            order(&one_to_20),
            [5, 10, 14, 20, 1, 6, 9, 13, 2, 17, 12, 7, 3, 18, 16, 11, 8, 19, 4, 15]
        );
        assert_eq!(
            order(&[-5, 5, -17, 17, 100, -100, 31, 32, 33]),
            [5, -100, 33, -5, 31, -17, 100, 32, 17]
        );
    }
}
