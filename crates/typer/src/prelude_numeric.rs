//! Numeric companion-object constants: `Int.MaxValue`, `Double.NaN`, etc.
//!
//! Verified against the real ABI with
//! `javap -cp /tmp/scala-rs-lib/scala-library-2.13.16.jar -s scala.Int$` (and
//! `Long$`/`Short$`/`Byte$`/`Char$`/`Double$`/`Float$`): each of these is a
//! nullary instance method on the companion object (`MODULE$.MaxValue()`),
//! not a static field. `Boolean$` carries no such constants (only
//! `box`/`unbox`/`toString`, out of scope here).
//!
//! Only wired when `library_abi` is set: the private runtime (`backend::runtime`,
//! used with `--no-scala-library`) has no `scala/Int$` etc. classfiles, so
//! adding these unconditionally would emit bytecode that references classes
//! that don't exist there. Gating means `--no-scala-library` correctly reports
//! "not a member of Int" instead of producing bytecode that fails at load time.

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

fn module(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    let cls = st.alloc(
        format!("{name}$"),
        owner,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        jvm,
    );
    let m = st.alloc(name, owner, SymKind::Module, Flags::MODULE, jvm);
    st.get_mut(m).ty = Type::ModuleRef(cls);
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    m
}

fn getter(st: &mut SymbolTable, owner: SymbolId, name: &str, ret: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Method, Flags::FINAL, "");
    st.get_mut(id).ty = Type::Method {
        paramss: Vec::new(),
        ret: Box::new(ret),
    };
    st.get_mut(id).intrinsic = Intrinsic::None;
    id
}

/// One companion module (e.g. `scala.Int$`) with a list of `(name, type)` nullary getters.
fn add_companion(
    st: &mut SymbolTable,
    scala_pkg: SymbolId,
    name: &str,
    jvm: &str,
    consts: &[(&str, Type)],
) {
    let m = module(st, scala_pkg, name, jvm);
    let cls = st.module_class_of(m);
    for (cname, ty) in consts {
        getter(st, cls, cname, ty.clone());
    }
}

pub fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let scala_pkg = st.scala_pkg;
    add_companion(
        st,
        scala_pkg,
        "Int",
        "scala/Int$",
        &[("MinValue", Type::Int), ("MaxValue", Type::Int)],
    );
    add_companion(
        st,
        scala_pkg,
        "Long",
        "scala/Long$",
        &[("MinValue", Type::Long), ("MaxValue", Type::Long)],
    );
    add_companion(
        st,
        scala_pkg,
        "Short",
        "scala/Short$",
        &[("MinValue", Type::Short), ("MaxValue", Type::Short)],
    );
    add_companion(
        st,
        scala_pkg,
        "Byte",
        "scala/Byte$",
        &[("MinValue", Type::Byte), ("MaxValue", Type::Byte)],
    );
    add_companion(
        st,
        scala_pkg,
        "Char",
        "scala/Char$",
        &[("MinValue", Type::Char), ("MaxValue", Type::Char)],
    );
    add_companion(
        st,
        scala_pkg,
        "Double",
        "scala/Double$",
        &[
            ("MinValue", Type::Double),
            ("MaxValue", Type::Double),
            ("MinPositiveValue", Type::Double),
            ("NaN", Type::Double),
            ("PositiveInfinity", Type::Double),
            ("NegativeInfinity", Type::Double),
        ],
    );
    add_companion(
        st,
        scala_pkg,
        "Float",
        "scala/Float$",
        &[
            ("MinValue", Type::Float),
            ("MaxValue", Type::Float),
            ("MinPositiveValue", Type::Float),
            ("NaN", Type::Float),
            ("PositiveInfinity", Type::Float),
            ("NegativeInfinity", Type::Float),
        ],
    );
}
