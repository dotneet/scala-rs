//! The rest of `scala.language`.
//!
//! `prelude.rs` declares the three feature flags the typer itself reacts to
//! (`dynamics`, `postfixOps`, `implicitConversions`). The others are still
//! importable names in 2.13, and `import scala.language.{implicitConversions,
//! existentials}` must not fail on the second selector, so they are declared
//! here too — together with `scala.language.experimental.macros`.
//!
//! They carry no behaviour: scala-rs does not gate existential types, refined
//! structural calls or higher-kinded types behind a feature import, so the
//! symbols exist purely so the import resolves. Anything scala-rs cannot
//! actually compile still reports its own error at the use site.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    let Some(language) = st
        .lookup_member(st.scala_pkg, "language")
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Module)
    else {
        return;
    };
    for feat in ["existentials", "higherKinds", "reflectiveCalls"] {
        add_feature(st, language, feat);
    }
    let experimental = experimental_module(st, language);
    add_feature(st, experimental, "macros");
}

/// `scala.language.experimental`, an object nested in `object language`.
fn experimental_module(st: &mut SymbolTable, language: SymbolId) -> SymbolId {
    let lang_cls = st.module_class_of(language);
    let module = st.alloc(
        "experimental",
        lang_cls,
        SymKind::Module,
        Flags::MODULE,
        "scala/language$experimental$",
    );
    let cls = st.alloc(
        "experimental$",
        lang_cls,
        SymKind::ModuleClass,
        Flags::MODULE,
        "scala/language$experimental$",
    );
    st.get_mut(module).ty = Type::ModuleRef(cls);
    st.get_mut(lang_cls).members.push(module);
    st.get_mut(language).members.push(module);
    module
}

/// One `implicit lazy val <name>` marker on `owner`'s module class.
fn add_feature(st: &mut SymbolTable, owner: SymbolId, name: &str) {
    let cls = st.module_class_of(owner);
    if !st.lookup_member(cls, name).is_empty() {
        return;
    }
    let id = st.alloc(
        name,
        cls,
        SymKind::Term,
        Flags::IMPLICIT.with(Flags::LAZY).with(Flags::FINAL),
        "",
    );
    st.get_mut(id).ty = Type::Boolean;
    if cls != owner {
        st.get_mut(cls).members.push(id);
    }
    st.get_mut(owner).members.push(id);
}
