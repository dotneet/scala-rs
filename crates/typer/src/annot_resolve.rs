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
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;

impl Typer {
    /// Resolve the annotations written on `tree` (a definition) and on its
    /// parameters and type parameters.
    pub(crate) fn resolve_annotation_types(&mut self, tree: &Tree) {
        if self.sigs_only {
            return;
        }
        let mut annots: Vec<(SymbolId, usize, Tree)> = Vec::new();
        let push_mods = |t: &Tree, out: &mut Vec<(SymbolId, usize, Tree)>| {
            let mods = match &t.kind {
                TreeKind::DefDef { mods, .. }
                | TreeKind::ValDef { mods, .. }
                | TreeKind::ClassDef { mods, .. }
                | TreeKind::ModuleDef { mods, .. }
                | TreeKind::TypeDef { mods, .. } => mods,
                _ => return,
            };
            out.extend(
                mods.annotations
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, annot)| (t.sym, index, annot)),
            );
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
        // A top-level template is checked after its package body has restored
        // the typer's current owner to the root.  Annotation names still live
        // in the scope *enclosing the annotated definition*, so anchor package
        // and inherited-name lookup at the definition symbol's owner.  Method
        // type parameters remain out of scope, as required, because their
        // owner is deliberately skipped here.
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        if !tree.sym.is_none() {
            let lexical_owner = self.st.get(tree.sym).owner;
            self.st.owner = lexical_owner;
            let mut enclosing = lexical_owner;
            while !enclosing.is_none() && !self.st.get(enclosing).is_class_like() {
                let next = self.st.get(enclosing).owner;
                if next == enclosing {
                    enclosing = SymbolId::NONE;
                    break;
                }
                enclosing = next;
            }
            self.st.this_class = enclosing;
        }
        let saved = std::mem::replace(&mut self.resolving_annot, true);
        for (sym, index, annot) in &annots {
            if let Some(ty) = self.resolve_one_annotation(annot) {
                if !sym.is_none() {
                    // A source object has two symbols: its stable module and
                    // the module class that carries the classfile and pickle
                    // metadata. Namer copies the annotation to both before
                    // this pass resolves it, so update both copies together.
                    let targets = if self.st.get(*sym).kind == SymKind::Module {
                        vec![*sym, self.st.module_class_of(*sym)]
                    } else {
                        vec![*sym]
                    };
                    for target in targets {
                        if let Some(saved) = self.st.get_mut(target).annotations.get_mut(*index) {
                            saved.ty = ty.clone();
                        }
                    }
                }
            }
        }
        self.resolving_annot = saved;
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
    }

    fn resolve_one_annotation(&mut self, a: &Tree) -> Option<Type> {
        // Synthesized annotations (`@SerialVersionUID` on a case class
        // companion, `@deprecated` copied onto an accessor) name their class
        // already; only what the source wrote is looked up.
        if a.span.is_dummy() {
            return None;
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
                    return None;
                }
                let mut t = Tree::dummy(TreeKind::AppliedTypeTree {
                    tpt: f.clone(),
                    args: targs.clone(),
                });
                t.span = fun.span;
                t
            }
            TreeKind::Ident { .. } | TreeKind::Select { .. } => fun.clone(),
            _ => return None,
        };
        // Even in private-runtime mode, a source annotation may name a class
        // imported from the user's binary dependencies. Resolve it without
        // strict diagnostics there: the private runtime intentionally omits
        // many standard annotation classes, so an unresolved name must not
        // turn into a new source error just to improve the pickle.
        // Lenient name lookup is not enough: a qualified spelling such as
        // `@scala.specialized` still reports a missing package member, so the
        // speculative lookup drops whatever it reported.
        let ty = if self.library_abi {
            self.with_strict_sig_names(|s| s.tree_to_type(&tpt))
        } else {
            let mark = self.diags.len();
            let ty = self.tree_to_type(&tpt);
            self.diags.truncate(mark);
            ty
        };
        if ty.is_error() || (!self.library_abi && !is_annotation_class_type(&ty)) {
            return None;
        }
        if self.library_abi {
            for arg in args {
                self.resolve_classof_types(arg);
            }
        }
        Some(ty)
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

fn is_annotation_class_type(ty: &Type) -> bool {
    match ty {
        Type::Class { .. } => true,
        Type::Applied { ctor, .. } => matches!(ctor.as_ref(), Type::Class { .. }),
        _ => false,
    }
}
