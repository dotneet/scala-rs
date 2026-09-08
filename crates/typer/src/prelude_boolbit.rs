//! The value-class members the hand-written prelude never declared:
//! `Boolean`'s three *bitwise* operators and the numeric `unary_+`.
//!
//! Audited against the released ABI rather than guessed at:
//!
//! ```text
//! javap -p -classpath scala-library-2.13.16.jar scala.Boolean
//!   public abstract boolean unary_$bang();
//!   public abstract boolean $eq$eq(boolean);   public abstract boolean $bang$eq(boolean);
//!   public abstract boolean $bar$bar(boolean); public abstract boolean $amp$amp(boolean);
//!   public abstract boolean $bar(boolean);     public abstract boolean $amp(boolean);
//!   public abstract boolean $up(boolean);
//! ```
//!
//! `prelude_anyval2::add_bool_members` had the first five. The last three --
//! `|`, `&`, `^` -- were missing, which is 15 errors in `src/library` alone
//! (`docs/scala-library.md`).
//!
//! **They are not spellings of `||` and `&&`.** `p & q` evaluates `q`
//! unconditionally; `p && q` does not. Emitting the short-circuiting form for
//! them would type-check, pass every classfile check, and silently change the
//! program's side effects -- so the codegen is `iand`/`ior`/`ixor` over both
//! operands, and `tests/fixtures/libprelude_boolbit.scala` *runs* with an
//! operand that prints, so the two forms are distinguishable at run time.
//!
//! `unary_+` is the identity on the widened receiver (`scala.Byte` declares
//! `public abstract int unary_$plus()`), which is why the result type follows
//! nsc's own widening -- `Int` for `Byte`/`Short`/`Char`/`Int`, and the
//! receiver's own type for `Long`/`Float`/`Double`.
//!
//! Neither group needs the library: both are pure JVM instruction sequences,
//! so they are installed in `--no-scala-library` mode too and nothing here is
//! gated on `library_abi`.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::SymbolId;
use scala_rs_parser::Type;

pub fn install(st: &mut SymbolTable) {
    let b = st.boolean_sym;
    for op in ["&", "|", "^"] {
        if has_member(st, b, op, &Type::Boolean) {
            continue;
        }
        prelude_method(
            st,
            b,
            op,
            vec![Type::Boolean],
            Type::Boolean,
            Intrinsic::BoolBin(op),
        );
    }
    // `unary_+`: the receiver's own value, after nsc's widening of the result.
    //
    // The intrinsic is the receiver's own unary family with a `"+"` payload,
    // not `Intrinsic::Identity`: on the bare `Select` a nullary `x.unary_+`
    // never reaches `gen_expr`'s `Identity` arm, so `Identity` left the
    // generic call path in place and emitted
    // `invokevirtual java.lang.Byte.unary_$plus()`, which does not exist --
    // it type-checked and died at run time, which is why the fixture runs.
    // `IntUn` / `LongUn` / `FloatUn` / `DoubleUn` all emit nothing after the
    // operand for an op they do not know, which is exactly the identity.
    for (owner, ret, ic) in [
        (st.byte_sym, Type::Int, Intrinsic::IntUn("+")),
        (st.short_sym, Type::Int, Intrinsic::IntUn("+")),
        (st.char_sym, Type::Int, Intrinsic::IntUn("+")),
        (st.int_sym, Type::Int, Intrinsic::IntUn("+")),
        (st.long_sym, Type::Long, Intrinsic::LongUn("+")),
        (st.float_sym, Type::Float, Intrinsic::FloatUn("+")),
        (st.double_sym, Type::Double, Intrinsic::DoubleUn("+")),
    ] {
        if has_nullary(st, owner, "unary_+") {
            continue;
        }
        prelude_method(st, owner, "unary_+", vec![], ret, ic);
    }
}

/// Already declared with exactly this parameter type?
fn has_member(st: &SymbolTable, owner: SymbolId, name: &str, arg: &Type) -> bool {
    st.get(owner).members.iter().any(|&m| {
        let s = st.get(m);
        s.name == name
            && matches!(s.kind, SymKind::Method)
            && matches!(&s.ty, Type::Method { paramss, .. }
                if paramss.first().map(|p| p.as_slice()) == Some(std::slice::from_ref(arg)))
    })
}

fn has_nullary(st: &SymbolTable, owner: SymbolId, name: &str) -> bool {
    st.get(owner).members.iter().any(|&m| {
        let s = st.get(m);
        s.name == name
            && matches!(s.kind, SymKind::Method)
            && matches!(&s.ty, Type::Method { paramss, .. }
                if paramss.first().is_none_or(|p| p.is_empty()))
    })
}
