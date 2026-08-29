//! The 7x7 `toByte` / `toShort` / `toChar` / `toInt` / `toLong` / `toFloat` /
//! `toDouble` tower.
//!
//! nsc declares all seven conversions on *each* of `Byte`, `Short`, `Char`,
//! `Int`, `Long`, `Float` and `Double` (they come from `AnyVal`'s numeric
//! value classes in `scala/Byte.scala` and friends, not from an implicit
//! `RichX`), so `3L.toByte`, `3.0.toChar` and `'a'.toDouble` are all plain
//! member calls. The hand-written prelude had eight of the forty-nine, and
//! every other one reported "value toX is not a member of ...".
//!
//! Each one is a pure JVM instruction sequence, so they are `Intrinsic`s
//! rather than calls into the library: that keeps them working on the private
//! runtime, where there is no `scala/runtime/RichLong` to delegate to. See
//! `gen::emit_num_conv` for the encoding of the payload.
//!
//! `install` deliberately skips any `toX` already declared on the class, so
//! the pre-existing `IntToLong` / `IntToByte` / `LongToFloat` / ... members
//! keep their own intrinsics and nothing is declared twice (an overload pair
//! with identical signatures would make every call ambiguous).

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::SymbolId;
use scala_rs_parser::Type;

/// Method suffix, JVM descriptor letter, result type. Ordered the way nsc
/// lists them, narrowest first.
const PRIMS: [(&str, &str, Type); 7] = [
    ("Byte", "B", Type::Byte),
    ("Short", "S", Type::Short),
    ("Char", "C", Type::Char),
    ("Int", "I", Type::Int),
    ("Long", "J", Type::Long),
    ("Float", "F", Type::Float),
    ("Double", "D", Type::Double),
];

/// `"IB"`, `"JD"`, ... as `&'static str`, because `Intrinsic` payloads are
/// `&'static str` and the pair is known at compile time.
const CODES: [[&str; 7]; 7] = [
    ["BB", "BS", "BC", "BI", "BJ", "BF", "BD"],
    ["SB", "SS", "SC", "SI", "SJ", "SF", "SD"],
    ["CB", "CS", "CC", "CI", "CJ", "CF", "CD"],
    ["IB", "IS", "IC", "II", "IJ", "IF", "ID"],
    ["JB", "JS", "JC", "JI", "JJ", "JF", "JD"],
    ["FB", "FS", "FC", "FI", "FJ", "FF", "FD"],
    ["DB", "DS", "DC", "DI", "DJ", "DF", "DD"],
];

pub fn install(st: &mut SymbolTable) {
    for (from, (_, _, _)) in PRIMS.iter().enumerate() {
        let owner = class_of(st, from);
        for (to, (name, _, ret)) in PRIMS.iter().enumerate() {
            let m = format!("to{name}");
            if has_nullary(st, owner, &m) {
                continue;
            }
            prelude_method(
                st,
                owner,
                &m,
                vec![],
                ret.clone(),
                Intrinsic::NumConv(CODES[from][to]),
            );
        }
    }
}

fn class_of(st: &SymbolTable, i: usize) -> SymbolId {
    match i {
        0 => st.byte_sym,
        1 => st.short_sym,
        2 => st.char_sym,
        3 => st.int_sym,
        4 => st.long_sym,
        5 => st.float_sym,
        _ => st.double_sym,
    }
}

/// Already declared directly on this class? Only the class's own members
/// matter: `Any.toString` is inherited, but no `toX` is.
fn has_nullary(st: &SymbolTable, owner: SymbolId, name: &str) -> bool {
    st.get(owner).members.iter().any(|&m| {
        let s = st.get(m);
        s.kind == SymKind::Method
            && s.name == name
            && matches!(&s.ty, Type::Method { paramss, .. } if paramss.is_empty())
    })
}
