//! Every annotation names a class, and nsc resolves that name like any other
//! type (`Typers.typedAnnotation`): an unknown one is `not found: type X`.
//!
//! ```scala
//! @NoSuchAnnot def f = 1            // not found: type NoSuchAnnot
//! @annotation.xxxxx class C         // type xxxxx is not a member of package annotation
//! @throws(classOf[Missing]) def g   // not found: type Missing
//! ```
//!
//! scala-rs only ever looked at an annotation's *spelling*, to recognise the
//! handful it interprets (`@tailrec`, `@switch`, `@native`, ...), so any
//! misspelt or unimported annotation compiled silently (scala/scala
//! `neg/t6558`, `neg/t6558b`, `neg/t6758`, `neg/t7259`, `neg/t3222`).
//!
//! The name is resolved in the scope enclosing the annotated definition, as
//! nsc does: `@throws[E] def f[E]` does not see the method's own `E`. A
//! definition's parameters and type parameters carry annotations too. Of the
//! arguments only the types written in a `classOf[T]` are resolved; the
//! argument expressions themselves are left to the passes that interpret
//! them.
//!
//! Only with the real library on the classpath: the private runtime declares
//! the few annotation classes it interprets and nothing else, so a
//! missing class there says nothing about the program.

use crate::check::Typer;
use scala_rs_parser::ast::*;

impl Typer {
    /// Resolve the annotations written on `tree` (a definition) and on its
    /// parameters and type parameters.
    pub(crate) fn resolve_annotation_types(&mut self, tree: &Tree) {
        if !self.library_abi || self.sigs_only {
            return;
        }
        let mut annots: Vec<Tree> = Vec::new();
        let push_mods = |t: &Tree, out: &mut Vec<Tree>| {
            let mods = match &t.kind {
                TreeKind::DefDef { mods, .. }
                | TreeKind::ValDef { mods, .. }
                | TreeKind::ClassDef { mods, .. }
                | TreeKind::ModuleDef { mods, .. }
                | TreeKind::TypeDef { mods, .. } => mods,
                _ => return,
            };
            out.extend(mods.annotations.iter().cloned());
        };
        push_mods(tree, &mut annots);
        match &tree.kind {
            TreeKind::DefDef {
                tparams, vparamss, ..
            }
            | TreeKind::ClassDef {
                tparams, vparamss, ..
            } => {
                for t in tparams.iter().chain(vparamss.iter().flatten()) {
                    push_mods(t, &mut annots);
                }
            }
            _ => {}
        }
        for a in &annots {
            self.resolve_one_annotation(a);
        }
    }

    fn resolve_one_annotation(&mut self, a: &Tree) {
        // Synthesized annotations (`@SerialVersionUID` on a case class
        // companion, `@deprecated` copied onto an accessor) name their class
        // already; only what the source wrote is looked up.
        if a.span.is_dummy() {
            return;
        }
        let mut fun = a;
        let mut args: Vec<&Tree> = Vec::new();
        while let TreeKind::Apply { fun: f, args: xs } = &fun.kind {
            args.extend(xs.iter());
            fun = f;
        }
        let tpt = match &fun.kind {
            TreeKind::TypeApply {
                fun: f,
                args: targs,
            } => {
                if !matches!(f.kind, TreeKind::Ident { .. } | TreeKind::Select { .. }) {
                    return;
                }
                let mut t = Tree::dummy(TreeKind::AppliedTypeTree {
                    tpt: f.clone(),
                    args: targs.clone(),
                });
                t.span = fun.span;
                t
            }
            TreeKind::Ident { .. } | TreeKind::Select { .. } => fun.clone(),
            _ => return,
        };
        let ty = self.with_strict_sig_names(|s| s.tree_to_type(&tpt));
        if ty.is_error() {
            return;
        }
        for arg in args {
            self.resolve_classof_types(arg);
        }
    }

    /// The `T` of every `classOf[T]` inside an annotation argument.
    fn resolve_classof_types(&mut self, t: &Tree) {
        if let TreeKind::TypeApply { fun, args } = &t.kind {
            let is_classof = matches!(&fun.kind, TreeKind::Ident { name } if name == "classOf")
                || matches!(&fun.kind, TreeKind::Select { name, .. } if name == "classOf");
            if is_classof {
                for targ in args {
                    let _ = self.with_strict_sig_names(|s| s.tree_to_type(targ));
                }
                return;
            }
        }
        let mut kids = Vec::new();
        crate::macros::push_children(t, &mut kids);
        let kids: Vec<Tree> = kids.into_iter().cloned().collect();
        for k in &kids {
            self.resolve_classof_types(k);
        }
    }
}
