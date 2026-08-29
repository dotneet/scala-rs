//! Constructor alternatives the prelude was missing.
//!
//! Kept in its own module so the additions do not collide with sibling agents
//! editing `prelude.rs` / `prelude_coll.rs`. Only a single call
//! (`crate::prelude_ovl2::install`) is wired into `install_prelude`.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    add_array_buffer_ctors(st);
}

/// `scala.collection.mutable.ArrayBuffer` declares `def this()` and
/// `def this(initialSize: Int)`; `add_array_buffer` in `prelude.rs` declares
/// neither, so `new ArrayBuffer[R](g.length)` had no alternative to match.
/// Both are real 2.13.16 constructors (`<init>()V` and `<init>(I)V`), so this
/// claims nothing the classfile cannot back up.
fn add_array_buffer_ctors(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let buf = st
        .lookup_member(mutp, "ArrayBuffer")
        .into_iter()
        .find(|&id| st.get(id).kind == SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    if buf.is_none() {
        return;
    }
    let self_ty = Type::Class {
        sym: buf,
        args: st
            .get(buf)
            .tparams
            .iter()
            .map(|&t| Type::TypeParam(t))
            .collect(),
    };
    let declared: Vec<Vec<Type>> = st
        .get(buf)
        .members
        .iter()
        .copied()
        .filter(|&id| st.get(id).name == "<init>")
        .filter_map(|id| match &st.get(id).ty {
            Type::Method { paramss, .. } => Some(paramss.first().cloned().unwrap_or_default()),
            _ => None,
        })
        .collect();
    for params in [Vec::new(), vec![Type::Int]] {
        if declared.contains(&params) {
            continue;
        }
        prelude_method(st, buf, "<init>", params, self_ty.clone(), Intrinsic::None);
    }
}
