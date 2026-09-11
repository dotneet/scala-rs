//! Singleton type tests: `_: p.type` and `x.isInstanceOf[p.type]`.
//!
//! SLS 8.2 and nsc's `TypeTestTreeMaker`: a typed pattern `_: p.type` tests
//! `p eq x` (`p == x` when `p` is not an `AnyRef`), and erasure
//! (`SingletonInstanceCheck`) lowers `x.isInstanceOf[p.type]` to a comparison
//! too. scala-rs emitted an `instanceof` of `p`'s class, which matched every
//! instance of that class -- and, for `x.isInstanceOf[a.X.type]`, every
//! instance of `b.X` as well (run/t4577, run/t12312).
//!
//! The comparison needs `p` as a term, which a type tree does not carry: the
//! path is typed here and codegen (`gen_match::singleton_pattern_ref`) emits
//! it.
//!
//! Also here, for the same reason (erasure drops what codegen needs from the
//! type): a bare `classOf` with an inferred type argument is spelled out as a
//! `TypeApply`.

use crate::check::Typer;
use scala_rs_parser::ast::Type;
use scala_rs_parser::{Tree, TreeKind};

impl Typer {
    /// A bare `classOf` whose type argument came from the expected type
    /// (`val z: Class[C] = classOf`) is `classOf[C]`, the class constant --
    /// scalac 2.13.16 prints `class Test$C` for it (run/t4871). Written out
    /// as the `TypeApply` codegen folds; erasure would otherwise drop the
    /// `C` and leave a call to `Predef.classOf`, whose body answers `null`.
    /// A *selected* `Predef.classOf` stays that call in scalac as well.
    pub(crate) fn spell_out_inferred_class_of(&mut self, tree: &mut Tree) {
        if tree.sym.is_none()
            || !matches!(
                self.st.get(tree.sym).intrinsic,
                crate::symbol::Intrinsic::ClassOf
            )
        {
            return;
        }
        let Type::Class { args, .. } = &tree.ty else {
            return;
        };
        let [arg] = args.as_slice() else {
            return;
        };
        if arg.is_no_type() || arg.is_error() || matches!(arg, Type::TypeParam(_)) {
            return;
        }
        let mut targ = Tree::dummy(TreeKind::Empty);
        targ.span = tree.span;
        targ.ty = arg.clone();
        let ty = tree.ty.clone();
        let span = tree.span;
        let sym = tree.sym;
        let fun = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        *tree = Tree::dummy(TreeKind::TypeApply {
            fun: Box::new(fun),
            args: vec![targ],
        });
        tree.span = span;
        tree.ty = ty;
        tree.sym = sym;
    }

    /// Type the path of a singleton type tree (`p.type`, `this.type`) as a
    /// term. `ty` is what the tree denotes; only a tree that denotes a
    /// singleton type is touched -- an invalid path was already reported by
    /// `singleton_to_type`.
    pub(crate) fn type_singleton_type_ref(&mut self, tpt: &mut Tree, ty: &Type) {
        if !matches!(
            self.st.dealias(ty),
            Type::SingleType { .. } | Type::ThisType(_)
        ) {
            return;
        }
        match &mut tpt.kind {
            TreeKind::AnnotatedTypeTree { tpt: inner, .. } => {
                self.type_singleton_type_ref(inner, ty)
            }
            TreeKind::SingletonTypeTree { ref_ } if ref_.ty.is_no_type() => {
                self.type_expr(ref_, &Type::NoType);
            }
            _ => {}
        }
    }
}
