//! Put a named application's arguments back into the order they were written.
//!
//! Scala binds arguments to parameters by name, but still evaluates them left
//! to right *as the call was written* (SLS 6.6.1). The typer resolves the names
//! by permuting the argument list into parameter order, and the backend then
//! evaluates that permuted list — which is a different order whenever the call
//! actually reordered anything:
//!
//! ```scala
//! def f(a: Int, b: Int, c: Int) = ()
//! f(c = p("c"), a = p("a"), b = p("b"))
//! ```
//!
//! prints `c a b` under scalac and printed `a b c` here.
//!
//! nsc's `NamesDefaults.transformNamedApplication` lifts the arguments into
//! `val`s ahead of the call and passes the references in parameter order; this
//! pass does the same, over the typed tree:
//!
//! ```scala
//! { val x$1 = p("c"); val x$2 = p("a"); val x$3 = p("b"); f(x$2, x$3, x$1) }
//! ```
//!
//! Which applications need it is recorded by the typer in
//! [`SymbolTable::named_arg_order`], keyed by node id, so a call whose names
//! happened to be in parameter order already (`f(a = 1, b = 2)` — much the
//! commoner shape) is not touched at all.
//!
//! Three things are deliberately left where they are:
//!
//! * an argument that cannot be observed out of order — a literal, `this`, a
//!   reference to something immutable, a function literal (building the closure
//!   has no effect a later argument could see);
//! * an argument to a **by-name** parameter, which the call site does not
//!   evaluate at all. The typer strikes those slots out of the recorded order,
//!   because a `val` in front of the call would evaluate them eagerly;
//! * a slot the typer filled itself — a default or a searched implicit. nsc
//!   likewise evaluates those inside the application, after the written
//!   arguments.
//!
//! Like [`crate::default_recv`], only the **outermost** `Apply` of a chain may
//! be wrapped: `Apply { fun: Block, … }` leaves the backend with no callee
//! symbol. An inner clause that needs reordering is hoisted into that same
//! outermost block, which is still the right place — the clauses of a curried
//! call are evaluated in order, so clause 1's arguments precede clause 2's. The
//! receiver comes before either, so it is hoisted first when it is something
//! that computes.
//!
//! The pass runs after [`crate::default_recv`] and before `uncurry`, so a call
//! that also omitted defaults already has its receiver bound to a local by the
//! time this looks at it.

use scala_rs_parser::{Flags, Modifiers, SymbolId, Tree, TreeKind, Type};

use crate::lazy_local::children_mut;
use crate::symbol::{SymKind, SymbolTable};

/// Rewrite every application in `tree` whose named arguments were reordered.
pub fn restore_named_arg_order(file_index: usize, tree: &mut Tree, st: &mut SymbolTable) {
    if st.named_arg_order.is_empty() {
        return;
    }
    let mut p = Pass {
        st,
        file: file_index as u32,
        gensym: 0,
        owner: SymbolId::NONE,
    };
    p.walk(tree, true);
}

struct Pass<'a> {
    st: &'a mut SymbolTable,
    file: u32,
    gensym: u32,
    owner: SymbolId,
}

impl Pass<'_> {
    fn walk(&mut self, t: &mut Tree, outermost: bool) {
        let saved = self.owner;
        if !t.sym.is_none()
            && matches!(
                t.kind,
                TreeKind::DefDef { .. }
                    | TreeKind::ValDef { .. }
                    | TreeKind::ClassDef { .. }
                    | TreeKind::ModuleDef { .. }
            )
        {
            self.owner = t.sym;
        }
        match &mut t.kind {
            TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
                self.walk(fun, false);
                for a in args.iter_mut() {
                    self.walk(a, true);
                }
            }
            _ => {
                for c in children_mut(t) {
                    self.walk(c, true);
                }
            }
        }
        if outermost {
            self.rewrite(t);
        }
        self.owner = saved;
    }

    /// The clauses of the chain at `t` that were reordered, innermost first —
    /// which is the order they are evaluated in.
    fn reordered_clauses(&self, t: &Tree) -> Vec<(u32, Vec<Option<usize>>)> {
        fn walk(p: &Pass<'_>, t: &Tree, out: &mut Vec<(u32, Vec<Option<usize>>)>) {
            match &t.kind {
                TreeKind::Apply { fun, args } => {
                    walk(p, fun, out);
                    if let Some(order) = p.st.named_arg_order.get(&(p.file, t.id.0)) {
                        // A node id is only unique within its own file, and a
                        // synthesized tree may have inherited one. Anything
                        // that does not line up with what was recorded is left
                        // alone.
                        if order.len() <= args.len() {
                            out.push((t.id.0, order.clone()));
                        }
                    }
                }
                TreeKind::TypeApply { fun, .. } => walk(p, fun, out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(self, t, &mut out);
        out
    }

    fn rewrite(&mut self, t: &mut Tree) {
        if !matches!(t.kind, TreeKind::Apply { .. }) {
            return;
        }
        let clauses = self.reordered_clauses(t);
        if clauses.is_empty() {
            return;
        }
        let mut stats: Vec<Tree> = Vec::new();
        // The receiver is evaluated before any argument, so it has to be bound
        // first once anything moves in front of it.
        let qual_ty = innermost_qual(t)
            .filter(|q| computed(q))
            .map(|q| (q.span, q.ty.clone()));
        if let Some((span, ty)) = qual_ty {
            if ty.is_no_type() || ty.is_error() {
                return;
            }
            let (sym, ident) = self.fresh_local("qual$named", span, &ty);
            let Some(q) = innermost_qual_mut(t) else {
                return;
            };
            let recv = std::mem::replace(q, ident);
            stats.push(self.val_def(sym, span, ty, recv));
        }
        for (id, order) in clauses {
            // Source position -> the parameter slot it ended up in.
            let mut by_source: Vec<(usize, usize)> = order
                .iter()
                .enumerate()
                .filter_map(|(slot, src)| src.map(|s| (s, slot)))
                .collect();
            by_source.sort_unstable();
            for (_, slot) in by_source {
                let Some(args) = clause_args_mut(t, id) else {
                    continue;
                };
                let arg = &args[slot];
                let ty = arg.ty.clone();
                let span = arg.span;
                if ty.is_no_type() || ty.is_error() || self.safe_where_it_stands(arg) {
                    continue;
                }
                let (sym, ident) = self.fresh_local("x$named", span, &ty);
                let Some(args) = clause_args_mut(t, id) else {
                    continue;
                };
                let bound = std::mem::replace(&mut args[slot], ident);
                stats.push(self.val_def(sym, span, ty, bound));
            }
        }
        if stats.is_empty() {
            return;
        }
        let call = std::mem::replace(t, Tree::dummy(TreeKind::Empty));
        let call_ty = call.ty.clone();
        let call_span = call.span;
        let call_id = call.id;
        *t = Tree {
            id: call_id,
            span: call_span,
            kind: TreeKind::Block {
                stats,
                expr: Box::new(call),
            },
            ty: call_ty,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
    }

    /// A fresh local symbol of the given type, and an `Ident` that reads it.
    fn fresh_local(
        &mut self,
        prefix: &str,
        span: scala_rs_span::Span,
        ty: &Type,
    ) -> (SymbolId, Tree) {
        self.gensym += 1;
        let name = format!("{prefix}${}", self.gensym);
        let sym = self.st.alloc(
            name.clone(),
            self.owner,
            SymKind::Term,
            Flags::SYNTHETIC,
            "",
        );
        self.st.get_mut(sym).ty = ty.clone();
        let mut ident = Tree::dummy(TreeKind::Ident { name });
        ident.span = span;
        ident.sym = sym;
        ident.ty = ty.clone();
        (sym, ident)
    }

    /// Whether moving `t` past another argument cannot be observed.
    fn safe_where_it_stands(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::Literal { .. } | TreeKind::This { .. } | TreeKind::Super { .. } => true,
            // Building a closure has no effect another argument could see, and
            // leaving lambdas alone keeps lambda-lift looking at the shapes it
            // already knows.
            TreeKind::Function { .. } => true,
            // A `var` read can see a later argument's assignment; anything
            // else a bare name denotes cannot change.
            TreeKind::Ident { .. } => {
                !t.sym.is_none() && !self.st.get(t.sym).flags.contains(Flags::MUTABLE)
            }
            TreeKind::Typed { expr, .. } => self.safe_where_it_stands(expr),
            _ => false,
        }
    }

    /// The `val` binding `sym` to `rhs`, already typed.
    fn val_def(&self, sym: SymbolId, span: scala_rs_span::Span, ty: Type, rhs: Tree) -> Tree {
        let mut vd = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::SYNTHETIC),
            name: self.st.get(sym).name.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(rhs),
        });
        vd.span = span;
        vd.sym = sym;
        vd.ty = ty;
        vd
    }
}

/// The argument list of the clause in the chain at `t` whose node id is `id`.
fn clause_args_mut(t: &mut Tree, id: u32) -> Option<&mut Vec<Tree>> {
    match &mut t.kind {
        TreeKind::Apply { fun, args } => {
            if t.id.0 == id {
                Some(args)
            } else {
                clause_args_mut(fun, id)
            }
        }
        TreeKind::TypeApply { fun, .. } => clause_args_mut(fun, id),
        _ => None,
    }
}

fn computed(t: &Tree) -> bool {
    match &t.kind {
        TreeKind::Apply { .. }
        | TreeKind::New { .. }
        | TreeKind::Block { .. }
        | TreeKind::If { .. }
        | TreeKind::Match { .. }
        | TreeKind::Try { .. } => true,
        TreeKind::Typed { expr, .. } => computed(expr),
        _ => false,
    }
}

fn innermost_qual(t: &Tree) -> Option<&Tree> {
    match &t.kind {
        TreeKind::Select { qual, .. } => Some(qual),
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => innermost_qual(fun),
        _ => None,
    }
}

fn innermost_qual_mut(t: &mut Tree) -> Option<&mut Tree> {
    match &mut t.kind {
        TreeKind::Select { qual, .. } => Some(qual),
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => innermost_qual_mut(fun),
        _ => None,
    }
}
