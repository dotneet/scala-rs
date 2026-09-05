//! `java.lang.ClassLoader`, and `Class#getClassLoader(): ClassLoader`.
//!
//! `scala.reflect.api.JavaUniverse#runtimeMirror(loader: ClassLoader):
//! JavaMirror` is a completely ordinary method with real bytecode (confirmed
//! with `javap -p scala.reflect.api.JavaUniverse` against scala-reflect.jar
//! 2.13.16), so the general pickle-supply path (`PickleSupply::install`, via
//! `complete_named`) should install it exactly like any other jar member.
//! What actually happened is that it never got that far: its parameter type
//! `java.lang.ClassLoader` has no symbol at all, because nothing had declared
//! it, and `PickleSupply::ensure_class` refuses to build one for a class
//! outside `scala.` that has no *Scala* `ScalaSignature` pickle -- which a
//! plain JDK class such as `ClassLoader` never has (see
//! `docs/notes/macro-reflect-and-reify.md`'s note on this exact gap, and
//! `crates/typer/src/materialize.rs`'s doc comment on `runtimeMirror`).
//! Declining the whole parameter conversion declined the whole method, so
//! `runtimeMirror` was never a member of `JavaUniverse` at all -- not "found
//! but rejected", simply never installed.
//!
//! `java.lang.Class` sits in exactly the same family (also a plain JDK class
//! with no Scala pickle) and is not affected, because the prelude already
//! declares it by hand (`prelude::install`, `class(st, java_lang, "Class",
//! ...)`); `find_by_jvm` finds that declaration before `ensure_class` ever
//! reaches its "outside scala., no pickle" branch. `ClassLoader` needs the
//! same hand-written treatment, on the same reasoning: it is a real,
//! unremarkable JDK type, and every program that calls `runtimeMirror` at all
//! reaches it through `<something>.getClass.getClassLoader`, so
//! `Class#getClassLoader` has to exist too.
//!
//! This does not attempt the general fix (letting `ensure_class` fall back to
//! the ordinary Java classfile loader, `find_or_stub_java_class`, for *any*
//! pickle-less non-`scala.` class). That path is shared by a great deal of
//! signature conversion this slice did not audit, and the existing comment
//! there already explains a case (`ensure_class`'s "outside the library"
//! branch) where a broader change made a working member stop resolving.
//! Adding the one class actually needed is the narrower, safer fix.

use crate::prelude::{class, prelude_method};
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{Flags, Type};

/// Runs once, from the very end of `prelude::install`. Unconditional: unlike
/// `--scala-library`-gated members, `java.lang.ClassLoader` is a real JDK
/// class present in every JVM regardless of which scala-library backs a run,
/// so there is nothing to gate on `library_abi` here.
pub fn install(st: &mut SymbolTable) {
    let java_lang = crate::classpath::ensure_package(st, "java/lang");
    let loader = match crate::classpath::find_by_jvm(st, "java/lang/ClassLoader") {
        Some(id) => id,
        None => {
            let id = class(
                st,
                java_lang,
                "ClassLoader",
                "java/lang/ClassLoader",
                &[Type::AnyRef],
            );
            let f = st.get(id).flags.with(Flags::JAVA);
            st.get_mut(id).flags = f;
            id
        }
    };
    let Some(jclass) = crate::classpath::find_by_jvm(st, "java/lang/Class") else {
        return;
    };
    if st
        .lookup_member(jclass, "getClassLoader")
        .into_iter()
        .any(|m| st.get(m).kind == crate::symbol::SymKind::Method)
    {
        return;
    }
    prelude_method(
        st,
        jclass,
        "getClassLoader",
        vec![],
        Type::Class {
            sym: loader,
            args: vec![],
        },
        Intrinsic::None,
    );
}
