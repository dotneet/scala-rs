//! `scala.reflect.macros` — just enough to name a macro `Context`.
//!
//! A macro implementation's first parameter must be typed
//! `scala.reflect.macros.blackbox.Context` (or the whitebox one), and the typer
//! reads that type to decide which kind of macro it is. So the two class
//! symbols have to exist before a macro def can be checked at all.
//!
//! **These classes are deliberately empty.** None of `c.universe`, `c.Expr`,
//! `c.prefix`, … is installed yet, so a real macro implementation body still
//! fails to typecheck — loudly, with `value universe is not a member of
//! Context`, which is the honest answer. Populating them is phase 3 in
//! `docs/macros.md`, and it also needs path-dependent types (`c.Expr[Int]`)
//! and code generation against the scala-reflect ABI.

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::symbol::{SymKind, SymbolTable};

pub fn install_reflect_macros(st: &mut SymbolTable) {
    let reflect = crate::classpath::ensure_package(st, "scala/reflect");
    let macros = pkg(st, reflect, "macros", "scala/reflect/macros");
    let blackbox = pkg(st, macros, "blackbox", "scala/reflect/macros/blackbox");
    let whitebox = pkg(st, macros, "whitebox", "scala/reflect/macros/whitebox");
    ctx(st, blackbox, "scala/reflect/macros/blackbox/Context");
    ctx(st, whitebox, "scala/reflect/macros/whitebox/Context");
}

fn pkg(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    if let Some(existing) = st
        .lookup_member(owner, name)
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Package)
    {
        return existing;
    }
    st.alloc(name, owner, SymKind::Package, Flags::PACKAGE, jvm)
}

fn ctx(st: &mut SymbolTable, owner: SymbolId, jvm: &str) -> SymbolId {
    let id = st.alloc("Context", owner, SymKind::Class, Flags::EMPTY, jvm);
    st.get_mut(id).parents = vec![Type::AnyRef];
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}
