//! `ClassTag#wrap`, the one member the materialiser needs and the prelude did
//! not declare.
//!
//! nsc builds `ClassTag[Array[E]]` as `arrayType(<E's tag>)` whenever `E` has
//! no erasure of its own — `ClassTag(ScalaRunTime.arrayClass(<tag>
//! .runtimeClass))`, which is exactly what `ClassTag#wrap` is:
//!
//! ```text
//! public default scala.reflect.ClassTag<java.lang.Object> wrap();
//! ```
//!
//! (`javap -p scala/reflect/ClassTag.class`, scala-library 2.13.16; the
//! erased result is `ClassTag`, the pickled one is `ClassTag[Array[T]]`.)
//!
//! Without it, `def f[T: ClassTag] = classTag[Array[T]]` was answered with
//! `ClassTag(classOf[Array[T]])`, whose runtime class is the *element*'s —
//! `int` where scalac reports `[I`. See `crates/typer/src/check.rs`'s
//! `classtag_tree`.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::Type;

pub fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let Some(ct) = crate::classpath::find_by_jvm(st, "scala/reflect/ClassTag") else {
        return;
    };
    if !st.lookup_member(ct, "wrap").is_empty() {
        return;
    }
    let Some(&t) = st.get(ct).tparams.first() else {
        return;
    };
    prelude_method(
        st,
        ct,
        "wrap",
        vec![],
        Type::Class {
            sym: ct,
            args: vec![Type::Array(Box::new(Type::TypeParam(t)))],
        },
        Intrinsic::None,
    );
}
