//! Method-local `lazy val`s (nsc's `lazyvals` phase), between uncurry and
//! lambda-lift.
//!
//! A `lazy val` on a class or trait gets a field plus a `bitmap$0` bit and an
//! accessor; the backend already emits those. A *local* one has no instance to
//! hang a field on, so nsc rewrites it into a one-slot cell plus a nested
//! accessor `def`:
//!
//! ```scala
//! def f(n: Int) = {
//!   lazy val s = { println("mk"); "v" + n }
//!   s + s
//! }
//! ```
//!
//! becomes, before lambda-lift gets to it,
//!
//! ```scala
//! def f(n: Int) = {
//!   val s$lzy = new scala.runtime.LazyRef()      // no initialiser runs here
//!   def s(cell: LazyRef): String = <original rhs, guarded>
//!   s(s$lzy) + s(s$lzy)
//! }
//! ```
//!
//! and lambda-lift then hoists `s` onto the enclosing class, threading `n`
//! through as an extra parameter exactly as it does for any other nested def.
//! That is also what makes `lazy val a = b + 1; lazy val b = 2` work: `a`'s
//! body reads `b` as `b(b$lzy)`, so `b$lzy` is a free local of `a`'s accessor
//! and lambda-lift passes it in — the same `(LazyInt, LazyInt)` signature
//! scalac produces.
//!
//! The guard itself (`initialized` / `synchronized` / `initialize`) is emitted
//! by the backend from `SymbolTable::local_lazy_accessors`, not spelled out as
//! trees here: the cell's three members are never named by user code, and the
//! monitor needs an exception table the tree language cannot express.
//!
//! Before this pass the declaration site evaluated the right-hand side eagerly
//! — the program type-checked and ran, it just was not lazy.

use scala_rs_parser::TyBox;
use std::collections::HashMap;

use scala_rs_parser::{Flags, Modifiers, SymbolId, Tree, TreeKind, Type};

use crate::prelude_lazyref::cell_class;
use crate::symbol::{SymKind, SymbolTable};

/// Rewrite every method-local `lazy val` in `tree` into a cell plus a nested
/// accessor `def`. Class and trait members are left alone.
pub fn lazy_locals(tree: &mut Tree, st: &mut SymbolTable) {
    let mut p = Pass { st, gensym: 0 };
    p.walk(tree);
}

struct Pass<'a> {
    st: &'a mut SymbolTable,
    gensym: u32,
}

/// What a rewritten `lazy val` turned into: the call that replaces every
/// reference to it.
struct Accessor {
    sym: SymbolId,
    name: String,
    mty: Type,
    cell: SymbolId,
    cell_name: String,
    cell_ty: Type,
}

type Rewrite = HashMap<SymbolId, Accessor>;

impl Pass<'_> {
    fn walk(&mut self, tree: &mut Tree) {
        if let TreeKind::Block { .. } = &tree.kind {
            self.rewrite_block(tree);
        }
        for_each_child_mut(tree, &mut |c| self.walk(c));
    }

    fn rewrite_block(&mut self, tree: &mut Tree) {
        let TreeKind::Block { stats, .. } = &mut tree.kind else {
            return;
        };
        if !stats.iter().any(is_local_lazy_val) {
            return;
        }
        let mut map: Rewrite = HashMap::new();
        let mut i = 0;
        while i < stats.len() {
            if !is_local_lazy_val(&stats[i]) {
                i += 1;
                continue;
            }
            let Some((cell_val, accessor, vsym, entry)) = self.split(&mut stats[i]) else {
                i += 1;
                continue;
            };
            stats[i] = cell_val;
            stats.insert(i + 1, accessor);
            map.insert(vsym, entry);
            i += 2;
        }
        if map.is_empty() {
            return;
        }
        // The whole block, including the right-hand sides just moved into
        // accessors: that is how one local `lazy val` reads another.
        for_each_child_mut(tree, &mut |c| rewrite_refs(c, &map));
    }

    /// Split `lazy val x: T = rhs` into its cell `val` and its accessor `def`.
    /// Returns `(cell val, accessor def, original symbol, how to call it)`.
    fn split(&mut self, vd: &mut Tree) -> Option<(Tree, Tree, SymbolId, Accessor)> {
        let vsym = vd.sym;
        let span = vd.span;
        let ty = if vd.ty.is_no_type() {
            self.st.get(vsym).ty.clone()
        } else {
            vd.ty.clone()
        };
        let cell_cls = cell_class(self.st, &ty)?;
        let cell_ty = Type::Class {
            sym: cell_cls,
            args: vec![].into(),
        };
        let owner = self.st.get(vsym).owner;
        let name = self.st.get(vsym).name.clone();
        self.gensym += 1;

        let cell = self.st.alloc(
            format!("{name}$lzy{}", self.gensym),
            owner,
            SymKind::Term,
            Flags::SYNTHETIC,
            "",
        );
        self.st.get_mut(cell).ty = cell_ty.clone();
        self.st.record_local_lazy_cell(cell);

        // Named after the val, like nsc: lambda-lift appends its own `$n`, so
        // the emitted method comes out as `x$1` the way scalac's does.
        // `SymKind::Method`, like any other nested `def`: lambda-lift only
        // treats a `Term` owned by a method as a captured local, and the
        // accessor is called, not captured.
        let acc = self.st.alloc(
            name.clone(),
            owner,
            SymKind::Method,
            Flags::SYNTHETIC.with(Flags::FINAL),
            "",
        );
        let mty = Type::Method {
            paramss: vec![vec![cell_ty.clone()]].into(),
            ret: TyBox::new(ty.clone()),
        };
        self.st.get_mut(acc).ty = mty.clone();
        self.st.get_mut(acc).params = vec![cell];
        self.st.get_mut(acc).paramss = vec![vec![cell]];
        self.st.record_local_lazy_accessor(acc, cell);

        let TreeKind::ValDef { mods, rhs, .. } = &mut vd.kind else {
            return None;
        };
        let mut rhs = std::mem::replace(rhs, Box::new(Tree::dummy(TreeKind::Empty)));
        // `lazy val x = { if (c) return 1; 2 }` returns out of the *enclosing*
        // method, and that `return` is about to move into the accessor. The
        // backend decides whether a method needs the `NonLocalReturnControl`
        // handler by looking at its own body, which will no longer show it.
        let mut returns = Vec::new();
        collect_return_targets(&mut rhs, &mut returns);
        for r in returns {
            self.st.record_local_lazy_nlr(r);
        }
        let mut cell_mods = Modifiers::new(mods.flags);
        cell_mods.flags = cell_mods.flags.with(Flags::SYNTHETIC);
        cell_mods.flags.set(Flags::LAZY, false);

        // The cell `val` carries no right-hand side: the backend gives it the
        // `new scala/runtime/Lazy…()` that replaces the eager initialiser.
        let mut cell_val = Tree::dummy(TreeKind::ValDef {
            mods: cell_mods,
            name: self.st.get(cell).name.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(Tree::dummy(TreeKind::Empty)),
        });
        cell_val.span = span;
        cell_val.sym = cell;
        cell_val.ty = cell_ty.clone();

        let mut param = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::PARAM),
            name: self.st.get(cell).name.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(Tree::dummy(TreeKind::Empty)),
        });
        param.span = span;
        param.sym = cell;
        param.ty = cell_ty;

        let mut accessor = Tree::dummy(TreeKind::DefDef {
            // Public, not `private` as scalac's is: scalac's lambdas are
            // `invokedynamic` bodies on the same class, ours are separate
            // anonymous classes, and one of those calling a private accessor
            // is an `IllegalAccessError`.
            mods: Modifiers::new(Flags::SYNTHETIC.with(Flags::FINAL)),
            name,
            tparams: vec![],
            vparamss: vec![vec![param]],
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs,
        });
        accessor.span = span;
        accessor.sym = acc;
        accessor.ty = mty.clone();

        let entry = Accessor {
            sym: acc,
            name: self.st.get(acc).name.clone(),
            mty,
            cell,
            cell_name: self.st.get(cell).name.clone(),
            cell_ty: self.st.get(cell).ty.clone(),
        };
        Some((cell_val, accessor, vsym, entry))
    }
}

/// Methods `tree` `return`s out of. A `return` inside a nested `def` targets
/// that `def` and carries its symbol, so no filtering is needed here.
fn collect_return_targets(tree: &mut Tree, out: &mut Vec<SymbolId>) {
    if let TreeKind::Return { .. } = &tree.kind {
        if !tree.sym.is_none() && !out.contains(&tree.sym) {
            out.push(tree.sym);
        }
    }
    for_each_child_mut(tree, &mut |c| collect_return_targets(c, out));
}

fn is_local_lazy_val(t: &Tree) -> bool {
    match &t.kind {
        TreeKind::ValDef { mods, rhs, .. } => {
            mods.flags.contains(Flags::LAZY) && !rhs.is_empty() && !t.sym.is_none()
        }
        _ => false,
    }
}

/// `x` -> `x(x$lzy)` everywhere below `tree`. lambda-lift renames the accessor
/// to `x$1` and splices in whatever else the right-hand side captured.
fn rewrite_refs(tree: &mut Tree, map: &Rewrite) {
    if let TreeKind::Ident { .. } = &tree.kind {
        if let Some(a) = map.get(&tree.sym) {
            let span = tree.span;
            let ty = if tree.ty.is_no_type() {
                a.mty.result().clone()
            } else {
                tree.ty.clone()
            };
            let mut fun = Tree::dummy(TreeKind::Ident {
                name: a.name.clone(),
            });
            fun.span = span;
            fun.sym = a.sym;
            fun.ty = a.mty.clone();
            let mut arg = Tree::dummy(TreeKind::Ident {
                name: a.cell_name.clone(),
            });
            arg.span = span;
            arg.sym = a.cell;
            arg.ty = a.cell_ty.clone();
            let mut apply = Tree::dummy(TreeKind::Apply {
                fun: Box::new(fun),
                args: vec![arg],
            });
            apply.span = span;
            // Lowering a stable identifier must retain its pattern meaning.
            apply.stable_pat = tree.stable_pat;
            apply.sym = a.sym;
            apply.ty = ty;
            *tree = apply;
            return;
        }
    }
    for_each_child_mut(tree, &mut |c| rewrite_refs(c, map));
}

/// Every subtree that can hold an expression, handed to `v` in order.
/// Deliberately exhaustive so a new `TreeKind` does not silently stop being
/// visited. Allocates nothing, where [`children_mut`] builds a vector per
/// node: the passes that walk every tree spent a fifth of their time there.
pub(crate) fn for_each_child_mut<'a>(t: &'a mut Tree, v: &mut dyn FnMut(&'a mut Tree)) {
    match &mut t.kind {
        TreeKind::PackageDef { pid, stats } => {
            v(pid);
            stats.iter_mut().for_each(&mut *v);
        }
        TreeKind::Import { expr, .. } => v(expr),
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            tparams.iter_mut().for_each(&mut *v);
            for c in vparamss.iter_mut() {
                c.iter_mut().for_each(&mut *v);
            }
            impl_.parents.iter_mut().for_each(&mut *v);
            if let Some(t) = impl_.self_tpt.as_mut() {
                v(t);
            }
            impl_.body.iter_mut().for_each(&mut *v);
        }
        TreeKind::ModuleDef { impl_, .. } => {
            impl_.parents.iter_mut().for_each(&mut *v);
            if let Some(t) = impl_.self_tpt.as_mut() {
                v(t);
            }
            impl_.body.iter_mut().for_each(&mut *v);
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            v(tpt);
            v(rhs);
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            tparams.iter_mut().for_each(&mut *v);
            for c in vparamss.iter_mut() {
                c.iter_mut().for_each(&mut *v);
            }
            v(tpt);
            v(rhs);
        }
        TreeKind::MacroRhs { impl_ref } => v(impl_ref),
        TreeKind::TypeDef {
            tparams,
            rhs,
            lo,
            hi,
            views,
            ctx_bounds,
            ..
        } => {
            tparams.iter_mut().for_each(&mut *v);
            v(rhs);
            if let Some(t) = lo.as_mut() {
                v(t);
            }
            if let Some(t) = hi.as_mut() {
                v(t);
            }
            views.iter_mut().for_each(&mut *v);
            ctx_bounds.iter_mut().for_each(&mut *v);
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            params.iter_mut().for_each(&mut *v);
            v(rhs);
        }
        TreeKind::Block { stats, expr } => {
            stats.iter_mut().for_each(&mut *v);
            v(expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            v(cond);
            v(thenp);
            v(elsep);
        }
        TreeKind::Match { selector, cases } => {
            v(selector);
            for c in cases.iter_mut() {
                v(&mut c.pat);
                v(&mut c.guard);
                v(&mut c.body);
            }
        }
        TreeKind::Function { vparams, body } => {
            vparams.iter_mut().for_each(&mut *v);
            v(body);
        }
        TreeKind::Assign { lhs, rhs } => {
            v(lhs);
            v(rhs);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
            v(cond);
            v(body);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => v(expr),
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            v(block);
            for c in catches.iter_mut() {
                v(&mut c.pat);
                v(&mut c.guard);
                v(&mut c.body);
            }
            v(finalizer);
        }
        TreeKind::New { tpt } => v(tpt),
        TreeKind::Typed { expr, tpt } => {
            v(expr);
            v(tpt);
        }
        TreeKind::TypeApply { fun, args }
        | TreeKind::Apply { fun, args }
        | TreeKind::UnApply { fun, args } => {
            v(fun);
            args.iter_mut().for_each(&mut *v);
        }
        TreeKind::Select { qual, .. } | TreeKind::SelectFromTypeTree { qual, .. } => v(qual),
        TreeKind::Bind { body, .. } => v(body),
        TreeKind::Star { elem } => v(elem),
        TreeKind::Alternative { trees } => trees.iter_mut().for_each(&mut *v),
        TreeKind::AppliedTypeTree { tpt, args } => {
            v(tpt);
            args.iter_mut().for_each(&mut *v);
        }
        TreeKind::SingletonTypeTree { ref_ } => v(ref_),
        TreeKind::AnnotatedTypeTree { tpt, annot } => {
            v(tpt);
            v(annot);
        }
        TreeKind::CompoundTypeTree {
            parents,
            refinements,
        } => {
            parents.iter_mut().for_each(&mut *v);
            refinements.iter_mut().for_each(&mut *v);
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            v(tpt);
            clauses.iter_mut().for_each(&mut *v);
        }
        TreeKind::InterpolatedString { args, .. } => args.iter_mut().for_each(&mut *v),
        TreeKind::Empty
        | TreeKind::Super { .. }
        | TreeKind::This { .. }
        | TreeKind::Ident { .. }
        | TreeKind::Literal { .. }
        | TreeKind::Wildcard
        | TreeKind::Unimplemented { .. } => {}
    }
}

/// [`for_each_child_mut`] collected, for a caller that needs the children
/// together.
pub(crate) fn children_mut(t: &mut Tree) -> Vec<&mut Tree> {
    let mut out: Vec<&mut Tree> = Vec::new();
    for_each_child_mut(t, &mut |c| out.push(c));
    out
}
