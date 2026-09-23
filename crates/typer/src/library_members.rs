//! Library members a source class overrides, completed so the backend can
//! bridge them.
//!
//! `pickle_supply` installs a library member only when a lookup misses, and a
//! member the class implements itself is found on the class: nothing ever
//! asks its generic library parent. The hand-written prelude declares
//! `scala.math.Numeric` with no members at all, so
//!
//! ```scala
//! class Num extends Numeric[Int] { def fromInt(x: Int) = x; ... }
//! ```
//!
//! reached the backend with no `Numeric.fromInt(Int): T` to bridge, and
//! `List(1, 2, 3).sum(new Num)` died with `AbstractMethodError` on
//! `fromInt(I)Ljava/lang/Object;`. nsc's erasure sees every overridden
//! member, whatever the program happened to select.

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;

impl Typer {
    /// Ask `class_id`'s library ancestors for each member name `body`
    /// declares, so the overridden declarations exist for bridge generation.
    ///
    /// Only for a class with a *generic* library ancestor: erasure bridges
    /// come from type parameters, and a case class's `Product` / `Equals`
    /// would otherwise cost a pickle probe per member of every case class.
    pub(crate) fn complete_overridden_library_members(
        &mut self,
        class_id: SymbolId,
        body: &[Tree],
    ) {
        if class_id.is_none() {
            return;
        }
        let bases: Vec<SymbolId> = crate::lin::linearize(&self.st, class_id)
            .into_iter()
            .skip(1)
            .collect();
        let generic_library_parent = bases.iter().any(|&base| {
            let s = self.st.get(base);
            s.kind == SymKind::Class && !s.tparams.is_empty() && s.jvm_name.starts_with("scala/")
        });
        if !generic_library_parent {
            return;
        }
        let mut names: Vec<String> = Vec::new();
        for tree in body {
            if !matches!(tree.kind, TreeKind::DefDef { .. } | TreeKind::ValDef { .. }) {
                continue;
            }
            if let Some(name) = tree.name() {
                if name != "<init>" && !names.iter().any(|n| n == name) {
                    names.push(name.to_owned());
                }
            }
        }
        for name in names {
            // Completion is additive only on a miss: asking a class that
            // already declares the name (a prelude `Iterator.next`) would
            // install the pickle's copy beside the hand-written one.
            let declared = bases.iter().any(|&base| {
                self.st
                    .get(base)
                    .members
                    .iter()
                    .any(|&m| self.st.get(m).name == name)
            });
            if declared {
                continue;
            }
            self.pickle
                .complete(&mut self.st, &mut self.binary, class_id, &name);
        }
    }
}
