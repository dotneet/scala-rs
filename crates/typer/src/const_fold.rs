//! nsc's `ConstantFolder`, as far as a `final val`'s inferred type needs it.
//!
//! An unannotated `final val` keeps the constant type of its right-hand side
//! (`final val N = 42` is `Int(42)`, SLS 4.1), and nsc folds primitive
//! operations on constants while typing, so `final val C = 1 + 2` is `Int(3)`
//! and `final val X = N * 2` is `Int(84)`. Those types are part of the
//! pickle, and every reference to such a value is replaced by the literal --
//! which is also why reading `K.N` never initializes `K`.
//!
//! Only what nsc's folder does is folded here: unary `-`/`+`/`~`/`!`, the
//! numeric conversions and the binary operators over the primitive
//! constants, plus `String + String`.

use scala_rs_parser::{Lit, Tree, TreeKind, Type};

/// The constant `tree` evaluates to, if it is one.
pub(crate) fn fold(tree: &Tree) -> Option<Lit> {
    match &tree.kind {
        TreeKind::Literal { lit } => {
            (!matches!(lit, Lit::Unit | Lit::Symbol(_))).then(|| lit.clone())
        }
        TreeKind::Ident { .. } | TreeKind::Select { .. } if constant_of(&tree.ty).is_some() => {
            constant_of(&tree.ty)
        }
        TreeKind::Select { qual, name } => fold_unop(op_name(name), &fold(qual)?),
        TreeKind::Apply { fun, args } => {
            let TreeKind::Select { qual, name } = &fun.kind else {
                return None;
            };
            match args.as_slice() {
                [] => fold_unop(op_name(name), &fold(qual)?),
                [arg] => fold_binop(op_name(name), &fold(qual)?, &fold(arg)?),
                _ => None,
            }
        }
        _ => None,
    }
}

fn constant_of(ty: &Type) -> Option<Lit> {
    match ty {
        Type::Constant(lit) if !matches!(lit, Lit::Unit | Lit::Symbol(_)) => Some(lit.clone()),
        Type::Annotated { tpe, .. } => constant_of(tpe),
        _ => None,
    }
}

/// Operator names arrive either as written or already encoded.
fn op_name(name: &str) -> &str {
    match name {
        "$plus" => "+",
        "$minus" => "-",
        "$times" => "*",
        "$div" => "/",
        "$percent" => "%",
        "$less$less" => "<<",
        "$greater$greater" => ">>",
        "$greater$greater$greater" => ">>>",
        "$amp" => "&",
        "$bar" => "|",
        "$up" => "^",
        "$eq$eq" => "==",
        "$bang$eq" => "!=",
        "$less" => "<",
        "$less$eq" => "<=",
        "$greater" => ">",
        "$greater$eq" => ">=",
        "$amp$amp" => "&&",
        "$bar$bar" => "||",
        n => n,
    }
}

/// A numeric constant after binary numeric promotion's first step: `Char`
/// (the only sub-`Int` literal) becomes `Int`.
#[derive(Clone, Copy)]
enum Num {
    I(i32),
    L(i64),
    F(f32),
    D(f64),
}

impl Num {
    fn of(lit: &Lit) -> Option<Num> {
        Some(match lit {
            Lit::Int(n) => Num::I(*n),
            Lit::Char(c) => Num::I(*c as i32),
            Lit::Long(n) => Num::L(*n),
            Lit::Float(n) => Num::F(*n),
            Lit::Double(n) => Num::D(*n),
            _ => return None,
        })
    }
    fn rank(self) -> u8 {
        match self {
            Num::I(_) => 0,
            Num::L(_) => 1,
            Num::F(_) => 2,
            Num::D(_) => 3,
        }
    }
    fn to_rank(self, rank: u8) -> Num {
        match (self, rank) {
            (Num::I(n), 1) => Num::L(n as i64),
            (Num::I(n), 2) => Num::F(n as f32),
            (Num::I(n), 3) => Num::D(n as f64),
            (Num::L(n), 2) => Num::F(n as f32),
            (Num::L(n), 3) => Num::D(n as f64),
            (Num::F(n), 3) => Num::D(n as f64),
            (n, _) => n,
        }
    }
    fn as_f64(self) -> f64 {
        match self {
            Num::D(v) => v,
            _ => 0.0,
        }
    }
    fn lit(self) -> Lit {
        match self {
            Num::I(n) => Lit::Int(n),
            Num::L(n) => Lit::Long(n),
            Num::F(n) => Lit::Float(n),
            Num::D(n) => Lit::Double(n),
        }
    }
}

fn fold_unop(op: &str, x: &Lit) -> Option<Lit> {
    if let Lit::Boolean(b) = x {
        return (op == "unary_!").then_some(Lit::Boolean(!b));
    }
    let n = Num::of(x)?;
    // The numeric conversions fold too: `5.toLong` is `Long(5L)`. `Byte`
    // and `Short` have no literal here, so those two stay unfolded.
    let wide = match n {
        Num::I(v) => v as f64,
        Num::L(v) => v as f64,
        Num::F(v) => v as f64,
        Num::D(v) => v,
    };
    match op {
        "toInt" => {
            return Some(Lit::Int(match n {
                Num::I(v) => v,
                Num::L(v) => v as i32,
                _ => wide as i32,
            }))
        }
        "toLong" => {
            return Some(Lit::Long(match n {
                Num::I(v) => v as i64,
                Num::L(v) => v,
                _ => wide as i64,
            }))
        }
        "toFloat" => {
            return Some(Lit::Float(match n {
                Num::F(v) => v,
                Num::I(v) => v as f32,
                Num::L(v) => v as f32,
                Num::D(v) => v as f32,
            }))
        }
        "toDouble" => return Some(Lit::Double(n.to_rank(3).as_f64())),
        "toChar" => {
            let code = match n {
                Num::I(v) => v as u16,
                Num::L(v) => v as u16,
                _ => wide as i32 as u16,
            };
            return char::from_u32(code as u32).map(Lit::Char);
        }
        _ => {}
    }
    Some(match (op, n) {
        ("unary_+", n) => n.lit(),
        ("unary_-", Num::I(v)) => Lit::Int(v.wrapping_neg()),
        ("unary_-", Num::L(v)) => Lit::Long(v.wrapping_neg()),
        ("unary_-", Num::F(v)) => Lit::Float(-v),
        ("unary_-", Num::D(v)) => Lit::Double(-v),
        ("unary_~", Num::I(v)) => Lit::Int(!v),
        ("unary_~", Num::L(v)) => Lit::Long(!v),
        _ => return None,
    })
}

fn fold_binop(op: &str, x: &Lit, y: &Lit) -> Option<Lit> {
    // `String.+` takes `Any`, and adapting a constant argument to `Any`
    // drops its constant type: nsc folds `"a" + "b"` to `String("ab")` but
    // leaves `"n=" + 1` (and `1 + "a"`) a plain `String`.
    if let (Lit::String(a), Lit::String(b)) = (x, y) {
        return (op == "+").then(|| Lit::String(format!("{a}{b}")));
    }
    if matches!(x, Lit::String(_)) || matches!(y, Lit::String(_)) {
        return None;
    }
    if let (Lit::Boolean(a), Lit::Boolean(b)) = (x, y) {
        let (a, b) = (*a, *b);
        return Some(Lit::Boolean(match op {
            "&&" | "&" => a && b,
            "||" | "|" => a || b,
            "^" | "!=" => a != b,
            "==" => a == b,
            _ => return None,
        }));
    }
    let (a, b) = (Num::of(x)?, Num::of(y)?);
    if matches!(op, "<<" | ">>" | ">>>") {
        // A shift has the type of its (promoted) left operand.
        let count = match b {
            Num::I(c) => c as i64,
            Num::L(c) => c,
            _ => return None,
        };
        return Some(match a {
            Num::I(v) => {
                let c = (count & 31) as u32;
                Lit::Int(match op {
                    "<<" => v.wrapping_shl(c),
                    ">>" => v.wrapping_shr(c),
                    _ => ((v as u32) >> c) as i32,
                })
            }
            Num::L(v) => {
                let c = (count & 63) as u32;
                Lit::Long(match op {
                    "<<" => v.wrapping_shl(c),
                    ">>" => v.wrapping_shr(c),
                    _ => ((v as u64) >> c) as i64,
                })
            }
            _ => return None,
        });
    }
    let rank = a.rank().max(b.rank());
    let (a, b) = (a.to_rank(rank), b.to_rank(rank));
    macro_rules! cmp {
        ($a:expr, $b:expr) => {
            match op {
                "==" => Some(Lit::Boolean($a == $b)),
                "!=" => Some(Lit::Boolean($a != $b)),
                "<" => Some(Lit::Boolean($a < $b)),
                "<=" => Some(Lit::Boolean($a <= $b)),
                ">" => Some(Lit::Boolean($a > $b)),
                ">=" => Some(Lit::Boolean($a >= $b)),
                _ => None,
            }
        };
    }
    match (a, b) {
        (Num::I(a), Num::I(b)) => Some(Lit::Int(match op {
            "+" => a.wrapping_add(b),
            "-" => a.wrapping_sub(b),
            "*" => a.wrapping_mul(b),
            "/" if b != 0 => a.wrapping_div(b),
            "%" if b != 0 => a.wrapping_rem(b),
            "&" => a & b,
            "|" => a | b,
            "^" => a ^ b,
            _ => return cmp!(a, b),
        })),
        (Num::L(a), Num::L(b)) => Some(Lit::Long(match op {
            "+" => a.wrapping_add(b),
            "-" => a.wrapping_sub(b),
            "*" => a.wrapping_mul(b),
            "/" if b != 0 => a.wrapping_div(b),
            "%" if b != 0 => a.wrapping_rem(b),
            "&" => a & b,
            "|" => a | b,
            "^" => a ^ b,
            _ => return cmp!(a, b),
        })),
        (Num::F(a), Num::F(b)) => Some(Lit::Float(match op {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => a / b,
            "%" => a % b,
            _ => return cmp!(a, b),
        })),
        (Num::D(a), Num::D(b)) => Some(Lit::Double(match op {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => a / b,
            "%" => a % b,
            _ => return cmp!(a, b),
        })),
        _ => None,
    }
}
