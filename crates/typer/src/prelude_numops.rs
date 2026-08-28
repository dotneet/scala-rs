//! Mixed-type operators on the numeric value classes.
//!
//! `Int` declares `<(x: Int)`, `<(x: Long)`, `<(x: Float)` and `<(x: Double)`
//! in `scala.Int`, so `3 < math.Pi` type-checks with the *receiver* widened.
//! The hand-written prelude only had the same-type ones plus a few arithmetic
//! pairs, so a mixed comparison found no overload at all.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::SymbolId;
use scala_rs_parser::Type;

/// Wide to narrow, so the wider of two operand types wins.
const RANKED: &[(u8, &str)] = &[(0, "Int"), (1, "Long"), (2, "Float"), (3, "Double")];

const ARITH: &[&str] = &["+", "-", "*", "/", "%"];
const COMPARE: &[&str] = &["==", "!=", "<", "<=", ">", ">="];

pub fn install(st: &mut SymbolTable) {
    for &(lr, _) in RANKED {
        let recv = class_of(st, lr);
        for &(rr, _) in RANKED {
            let arg = ty_of(rr);
            let wide = lr.max(rr);
            let wide_ty = ty_of(wide);
            for op in ARITH {
                if has_member(st, recv, op, &arg) {
                    continue;
                }
                prelude_method(
                    st,
                    recv,
                    op,
                    vec![arg.clone()],
                    wide_ty.clone(),
                    bin(wide, op),
                );
            }
            for op in COMPARE {
                if has_member(st, recv, op, &arg) {
                    continue;
                }
                prelude_method(
                    st,
                    recv,
                    op,
                    vec![arg.clone()],
                    Type::Boolean,
                    bin(wide, op),
                );
            }
        }
    }
}

fn class_of(st: &SymbolTable, rank: u8) -> SymbolId {
    match rank {
        0 => st.int_sym,
        1 => st.long_sym,
        2 => st.float_sym,
        _ => st.double_sym,
    }
}

fn ty_of(rank: u8) -> Type {
    match rank {
        0 => Type::Int,
        1 => Type::Long,
        2 => Type::Float,
        _ => Type::Double,
    }
}

fn bin(rank: u8, op: &str) -> Intrinsic {
    let op = ARITH
        .iter()
        .chain(COMPARE.iter())
        .find(|o| **o == op)
        .copied()
        .unwrap_or("+");
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
