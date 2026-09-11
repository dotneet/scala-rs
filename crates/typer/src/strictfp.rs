//! `@strictfp`: the backend gives every method under it `ACC_STRICT`
//! (`Gen::is_strictfp`, nsc `Symbol.isStrictFP`), reading the annotation by
//! the name written. That name has to denote a class, as nsc checks for any
//! annotation: a bare `@strictfp` with nothing imported is
//! "not found: type strictfp", not a strict method.

use crate::check::*;
use scala_rs_parser::ast::*;

pub(crate) fn is_strictfp_annot(path: &str) -> bool {
    matches!(
        path,
        "strictfp" | "annotation.strictfp" | "scala.annotation.strictfp"
    )
}

impl Typer {
    pub(crate) fn check_strictfp_annotation(&mut self, annot: &Tree) {
        let mut head = annot;
        while let TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } = &head.kind {
            head = fun;
        }
        let head = head.clone();
        let ty = self.with_strict_type_names(|s| s.tree_to_type(&head));
        if self.st.class_sym_of(&ty).is_none() && !ty.is_error() {
            self.error(
                head.span,
                format!("not found: type {}", head.annotation_path()),
            );
        }
    }
}
