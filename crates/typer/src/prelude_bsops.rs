//! `Byte`, `Short` and `Char` as operands of the numeric operators.
//!
//! `prelude_numops` builds the mixed-type operator table for the four *wide*
//! value classes, which is what nsc's `scala.Int` etc. declare. But nsc
//! declares the same table on `scala.Byte`, `scala.Short` and `scala.Char`,
//! and lists `Byte`/`Short`/`Char` among every class's operand types too --
//! `Byte.+(x: Byte): Int`, `Int.<(x: Short): Boolean`, `Double.*(x: Char):
//! Double` and so on. Without those, `b * 3`, `b < 20` and `sa(0) + sa(1)`
//! all reported "value * is not a member of Byte" / "no matching overload",
//! which is what kept `Byte` and `Short` from being usable as the JVM
//! primitives they erase to.
//!
//! On the JVM `Byte`, `Short` and `Char` are all `int`, so they share a
//! promotion rank with `Int` and the existing `IntBin` / `LongBin` /
//! `FloatBin` / `DoubleBin` intrinsics cover every combination; only the
//! *declarations* were missing.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::SymbolId;
use scala_rs_parser::Type;

/// Every operand type of the numeric tower, with the rank the JVM promotes it
/// to: `Byte`, `Short`, `Char` and `Int` are all rank 0 (`int`).
const OPERANDS: [(u8, Type); 7] = [
    (0, Type::Byte),
    (0, Type::Short),
    (0, Type::Char),
    (0, Type::Int),
    (1, Type::Long),
    (2, Type::Float),
    (3, Type::Double),
];

const ARITH: [&str; 5] = ["+", "-", "*", "/", "%"];
const COMPARE: [&str; 6] = ["==", "!=", "<", "<=", ">", ">="];
const BITWISE: [&str; 3] = ["&", "|", "^"];
const SHIFT: [&str; 3] = ["<<", ">>", ">>>"];

pub fn install(st: &mut SymbolTable) {
    for (lp, lt) in OPERANDS.iter() {
        for (rp, rt) in OPERANDS.iter() {
            // `prelude_numops` already covers rank-0-as-`Int` against the wide
            // classes; only the rows and columns that mention `Byte`, `Short`
            // or `Char` are new here.
            if !is_narrow(lt) && !is_narrow(rt) {
                continue;
            }
            let Some(recv) = class_of(st, lt) else {
                continue;
            };
            let wide = (*lp).max(*rp);
            for op in ARITH {
                add(st, recv, op, rt, ty_of(wide), bin(wide, op));
            }
            for op in COMPARE {
                add(st, recv, op, rt, Type::Boolean, bin(wide, op));
            }
            // `1.0 & 2` is not a Scala expression: bitwise needs two integrals.
            if *lp > 1 || *rp > 1 {
                continue;
            }
            for op in BITWISE {
                add(st, recv, op, rt, ty_of(wide), bin(wide, op));
            }
            // A shift's result and intrinsic follow the *left* operand; the
            // right one only says how far, and may be an `Int` or a `Long`.
            for op in SHIFT {
                add(st, recv, op, rt, ty_of(*lp), bin(*lp, op));
            }
        }
    }
    // `-b` and `~b` on a narrow integral widen to `Int`, exactly as `b + 1` does.
    for t in [Type::Byte, Type::Short, Type::Char] {
        let Some(recv) = class_of(st, &t) else {
            continue;
        };
        for (op, ic) in [
            ("unary_-", Intrinsic::IntUn("-")),
            ("unary_~", Intrinsic::IntUn("~")),
        ] {
            if st.lookup_member(recv, op).into_iter().any(
                |m| matches!(&st.get(m).ty, Type::Method { paramss, .. } if paramss.is_empty()),
            ) {
                continue;
            }
            prelude_method(st, recv, op, vec![], Type::Int, ic);
        }
    }
}

fn is_narrow(t: &Type) -> bool {
    matches!(t, Type::Byte | Type::Short | Type::Char)
}

fn add(st: &mut SymbolTable, recv: SymbolId, op: &str, arg: &Type, ret: Type, ic: Intrinsic) {
    if has_member(st, recv, op, arg) {
        return;
    }
    prelude_method(st, recv, op, vec![arg.clone()], ret, ic);
}

fn class_of(st: &SymbolTable, t: &Type) -> Option<SymbolId> {
    Some(match t {
        Type::Byte => st.byte_sym,
        Type::Short => st.short_sym,
        Type::Char => st.char_sym,
        Type::Int => st.int_sym,
        Type::Long => st.long_sym,
        Type::Float => st.float_sym,
        Type::Double => st.double_sym,
        _ => return None,
    })
}

fn ty_of(rank: u8) -> Type {
    match rank {
        0 => Type::Int,
        1 => Type::Long,
        2 => Type::Float,
        _ => Type::Double,
    }
}

fn bin(rank: u8, op: &'static str) -> Intrinsic {
    match rank {
        0 => Intrinsic::IntBin(op),
        1 => Intrinsic::LongBin(op),
        2 => Intrinsic::FloatBin(op),
        _ => Intrinsic::DoubleBin(op),
    }
}

/// Already declared with exactly this parameter type?
fn has_member(st: &SymbolTable, owner: SymbolId, name: &str, arg: &Type) -> bool {
    st.lookup_member(owner, name).into_iter().any(|m| {
        matches!(&st.get(m).ty, Type::Method { paramss, .. }
            if paramss.first().and_then(|c| c.first()) == Some(arg))
    })
}
