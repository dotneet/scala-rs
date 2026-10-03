//! The access of a `val`'s field.
//!
//! nsc keeps every field `private` and reads it through the accessor, and
//! reflection-driven libraries depend on that: Jackson treats a public field
//! as a property of its own, so swagger-core resolved a case class's schema
//! along another path than for scalac's class file (and failed on it). The
//! backend declares these fields `private` too, but it also reads a field
//! directly where nsc would call the accessor, from other classes of the
//! same run (a companion's `unapply`, a derived codec in another file). Once
//! every class of the run exists, [`widen_cross_class_fields`] makes each
//! field some other class refers to `public` again, in place: an access flag
//! is two bytes at a fixed place, so nothing else in the file moves.
//!
//! A class compiled in a later run only ever calls the accessor of a class
//! it reads from the class path, so the classes of one run are all there is
//! to look at.

use crate::classfile::{EmittedClass, ACC_PRIVATE, ACC_PUBLIC};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

/// What this pass reads of a class file.
struct Shape {
    this: String,
    super_name: Option<String>,
    /// Each field's name and the offset of its `access_flags`.
    fields: Vec<(String, usize)>,
    /// Every `CONSTANT_Fieldref`, as (class, field name).
    field_refs: Vec<(String, String)>,
}

fn u2(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]) as usize)
}

fn u4(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_be_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]) as usize)
}

fn parse(b: &[u8]) -> Option<Shape> {
    enum Entry {
        Utf8(String),
        Class(usize),
        FieldRef(usize, usize),
        NameAndType(usize),
        Other,
    }
    let count = u2(b, 8)?;
    let mut pool: Vec<Entry> = Vec::with_capacity(count);
    pool.push(Entry::Other);
    let mut i = 10;
    while pool.len() < count {
        let tag = *b.get(i)?;
        let (entry, size, wide) = match tag {
            1 => {
                let len = u2(b, i + 1)?;
                let bytes = b.get(i + 3..i + 3 + len)?;
                // Modified UTF-8 only differs from UTF-8 for NUL and
                // supplementary characters, which no class or field name
                // this pass compares holds.
                let s = String::from_utf8_lossy(bytes).into_owned();
                (Entry::Utf8(s), 3 + len, false)
            }
            3 | 4 => (Entry::Other, 5, false),
            5 | 6 => (Entry::Other, 9, true),
            7 => (Entry::Class(u2(b, i + 1)?), 3, false),
            8 | 16 | 19 | 20 => (Entry::Other, 3, false),
            9 => (Entry::FieldRef(u2(b, i + 1)?, u2(b, i + 3)?), 5, false),
            10 | 11 | 17 | 18 => (Entry::Other, 5, false),
            12 => (Entry::NameAndType(u2(b, i + 1)?), 5, false),
            15 => (Entry::Other, 4, false),
            _ => return None,
        };
        pool.push(entry);
        if wide {
            pool.push(Entry::Other);
        }
        i += size;
    }
    let utf8 = |k: usize| match pool.get(k) {
        Some(Entry::Utf8(s)) => Some(s.clone()),
        _ => None,
    };
    let class = |k: usize| match pool.get(k) {
        Some(Entry::Class(n)) => utf8(*n),
        _ => None,
    };
    let this = class(u2(b, i + 2)?)?;
    let super_index = u2(b, i + 4)?;
    let super_name = if super_index == 0 {
        None
    } else {
        class(super_index)
    };
    let interfaces = u2(b, i + 6)?;
    i += 8 + interfaces * 2;
    let field_count = u2(b, i)?;
    i += 2;
    let mut fields = Vec::with_capacity(field_count);
    for _ in 0..field_count {
        fields.push((utf8(u2(b, i + 2)?)?, i));
        let attrs = u2(b, i + 6)?;
        i += 8;
        for _ in 0..attrs {
            i += 6 + u4(b, i + 2)?;
        }
    }
    let mut field_refs = Vec::new();
    for entry in &pool {
        if let Entry::FieldRef(c, nt) = entry {
            let Some(Entry::NameAndType(n)) = pool.get(*nt) else {
                continue;
            };
            if let (Some(c), Some(n)) = (class(*c), utf8(*n)) {
                field_refs.push((c, n));
            }
        }
    }
    Some(Shape {
        this,
        super_name,
        fields,
        field_refs,
    })
}

/// Makes `public` every `private` field of `classes` that another class of
/// `classes` refers to, and answers the indices of the classes it changed.
pub fn widen_cross_class_fields(classes: &mut [EmittedClass]) -> Vec<usize> {
    let shapes: Vec<Option<Shape>> = classes.iter().map(|c| parse(&c.bytes)).collect();
    let by_name: HashMap<&str, usize> = shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|s| (s.this.as_str(), i)))
        .collect();
    // (class index, field offset) of each field to open up.
    let mut wanted: HashSet<(usize, usize)> = HashSet::default();
    for shape in shapes.iter().flatten() {
        for (owner, name) in &shape.field_refs {
            if *owner == shape.this {
                continue;
            }
            // A reference names the class it was resolved against; the
            // field itself may be declared by a superclass.
            let mut at = by_name.get(owner.as_str()).copied();
            while let Some(k) = at {
                let Some(s) = &shapes[k] else { break };
                if let Some((_, off)) = s.fields.iter().find(|(n, _)| n == name) {
                    if s.this != shape.this {
                        wanted.insert((k, *off));
                    }
                    break;
                }
                at = s
                    .super_name
                    .as_deref()
                    .and_then(|p| by_name.get(p).copied());
            }
        }
    }
    let mut changed = Vec::new();
    for (k, off) in wanted {
        let bytes = &mut classes[k].bytes;
        let acc = u16::from_be_bytes([bytes[off], bytes[off + 1]]);
        if acc & ACC_PRIVATE == 0 {
            continue;
        }
        let acc = (acc & !ACC_PRIVATE) | ACC_PUBLIC;
        bytes[off..off + 2].copy_from_slice(&acc.to_be_bytes());
        changed.push(k);
    }
    changed.sort_unstable();
    changed.dedup();
    changed
}
