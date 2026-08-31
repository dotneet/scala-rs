//! `scala.reflect.macros` — just enough to name a macro `Context`.
//!
//! A macro implementation's first parameter must be typed
//! `scala.reflect.macros.blackbox.Context` (or the whitebox one), and the typer
//! reads that type to decide which kind of macro it is. So the two class
//! symbols have to exist before a macro def can be checked at all.
//!
//! **These classes are deliberately empty**, and they are only installed when
//! the real ones cannot be reached ([`want_context_stub`]). None of
//! `c.universe`, `c.Expr`, `c.prefix`, … is on them, so a macro implementation
//! body compiled without scala-reflect.jar fails to typecheck — loudly, with
//! `value universe is not a member of Context`, which is the honest answer.
//!
//! With scala-reflect.jar on the classpath the real `Context` is read from its
//! pickle instead and those members resolve for real (`docs/macros.md` §7.6).
//! The stub used to be installed unconditionally and *shadowed* the real one,
//! so adding the jar changed nothing.

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::check::ClasspathClass;
use crate::symbol::{SymKind, SymbolTable};

/// `scala.reflect.macros.blackbox.Context` as the classfile names it.
const BLACKBOX_CONTEXT_JVM: &str = "scala/reflect/macros/blackbox/Context";

/// Whether the placeholder `Context` classes are wanted.
///
/// They are, unless the classpath has the real ones. Note that the *scala
/// library* jar is not enough: `scala.reflect.macros` lives in
/// scala-reflect.jar, which `--scala-library` does not imply.
pub fn want_context_stub(classpath: &[ClasspathClass]) -> bool {
    !classpath.iter().any(|c| c.jvm_name == BLACKBOX_CONTEXT_JVM)
}

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
    // `blackbox.Context` is a *trait*, and this symbol is the one the pickle
    // supply fills in when scala-reflect.jar is there (`ensure_class` answers
    // `find_by_jvm` with it rather than building a second one). Without the
    // flag the backend emitted `invokevirtual Context.universe()` and the
    // macro implementation died with `IncompatibleClassChangeError` the first
    // time it was actually run -- which nothing did until the engine landed.
    let flags = Flags::INTERFACE.with(Flags::TRAIT).with(Flags::ABSTRACT);
    let id = st.alloc("Context", owner, SymKind::Class, flags, jvm);
    st.get_mut(id).parents = vec![Type::AnyRef];
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}
