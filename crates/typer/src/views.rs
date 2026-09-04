//! Filling an implicit parameter of *function* type from an implicit
//! conversion (agent/durrange).
//!
//! SLS 7.2 / 6.26.5: an implicit parameter whose type is `A => B` is a **view**
//! request. nsc answers it out of the same pool it uses for `x: A` where a `B`
//! is required, and hands back the conversion **eta-expanded into a function
//! value** — for `def h[A](x: A, y: A)(implicit ev: A => Ordered[A])` real
//! scalac 2.13.16 emits
//!
//! ```text
//! h(3, 5)       // $anonfun$main$1(int)    = Predef.intWrapper(x)
//! h("a", "b")   // $anonfun$main$2(String) = Ordered.orderingToOrdered(x)(Ordering.String)
//! ```
//!
//! scala-rs had no such route. [`Typer::fill_implicit_params_in`] searched for
//! a *value* of type `A => B` and, failing that, tried only two hard-wired
//! shapes: [`Typer::identity_view`] (`A <: B`) and
//! [`Typer::array_wrap_view`]. An `implicit def` was never considered, so
//! every function-typed implicit parameter that a conversion should have
//! satisfied — and therefore every view bound `def f[A <% B]`, which desugars
//! to exactly that parameter — reported
//! `no implicit: could not find implicit value of type (Int) => Ordered[Int]`.
//!
//! [`Typer::conversion_view`] closes that. It is deliberately *not* about
//! `Ordered`: it asks the ordinary view search for a conversion `A => B` and,
//! if one exists, builds `(x$n: A) => x$n` and lets [`Typer::adapt`] insert
//! the conversion into the body — the same code path, and the same choice of
//! conversion, that `val b: B = (a: A)` goes through. Any `A => B` works.
//!
//! The lambda is typed under a diagnostics mark: if adapting the body turns
//! out to report anything, the attempt is rolled back and the caller reports
//! the missing implicit as before. Nothing is accepted that the view search
//! did not actually witness.

use scala_rs_parser::{Flags, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::check::Typer;

impl Typer {
    /// A function value for the implicit parameter `pt = A => B`, built from
    /// an implicit conversion `A => B`, or `None` when there is no such
    /// conversion.
    pub(crate) fn conversion_view(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        let Type::Function { params, ret } = pt else {
            return None;
        };
        if params.len() != 1 {
            return None;
        }
        let from = params[0].clone();
        let to = (**ret).clone();
        if [&from, &to]
            .iter()
            .any(|t| t.is_no_type() || t.is_error() || matches!(t, Type::Wildcard))
        {
            return None;
        }
        // A bare type parameter on either end would let every conversion in
        // scope claim the parameter; nsc will not infer a view for a type it
        // has not pinned down either.
        if matches!(from, Type::TypeParam(_)) && matches!(to, Type::TypeParam(_)) {
            return None;
        }
        self.warm_implicit_scope(&from);
        self.warm_implicit_scope(&to);
        // Only a single winner builds a view. Two equally specific
        // conversions are declined here and reported by the caller as
        // `no implicit` rather than `ambiguous implicit` — less precise than
        // nsc, but never more permissive.
        if !self.search_conversion(&from, &to).is_found() {
            return None;
        }
        let mut lam = self.view_identity_lambda(&from, &to, span);
        let mark = self.diags.len();
        self.type_expr(&mut lam, pt);
        self.adapt(&mut lam, pt);
        if self.diags.len() != mark || lam.ty.is_error() {
            self.diags.truncate(mark);
            return None;
        }
        Some(lam)
    }

    /// `(x$n: from) => x$n`, untyped in the body's expected type: `type_expr`
    /// re-types the body against `to` and `adapt` puts the conversion in.
    fn view_identity_lambda(&mut self, from: &Type, to: &Type, span: Span) -> Tree {
        self.gensym += 1;
        let pname = format!("x${}", self.gensym);
        let pid = self.st.alloc(
            &pname,
            self.st.owner,
            crate::symbol::SymKind::Term,
            Flags::PARAM.with(Flags::SYNTHETIC),
            "",
        );
        self.st.get_mut(pid).ty = from.clone();
        let ident = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: pname.clone(),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
            scala_ref: false,
        };
        let param = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::ValDef {
                mods: scala_rs_parser::Modifiers::new(Flags::PARAM),
                name: pname,
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
            scala_ref: false,
        };
        Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Function {
                vparams: vec![param],
                body: Box::new(ident),
            },
            ty: Type::Function {
                params: vec![from.clone()],
                ret: Box::new(to.clone()),
            },
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
        }
    }
}
