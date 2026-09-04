//! Evaluate the receiver of a call that omitted default arguments once.
//!
//! nsc's named/default translation (`NamesDefaults`) binds the qualifier to a
//! local before it calls anything:
//!
//! ```scala
//! mk().infer()
//! // becomes
//! { val qual$1 = mk()
//!   val x$1 = qual$1.infer$default$1()
//!   val x$2 = qual$1.infer$default$2()
//!   qual$1.infer(x$1, x$2) }
//! ```
//!
//! scala-rs's `default_getter_apply` splices a **clone** of the receiver into
//! every `name$default$n` call it builds, so the qualifier was evaluated once
//! per omitted default plus once for the call itself. That is a
//! miscompilation, not just waste — with
//!
//! ```scala
//! class Ops(val n: Int) { def infer(scope: Int = 0, deep: Boolean = false) = n }
//! def mk(): Ops = { println("recv"); new Ops(7) }
//! mk().infer()
//! ```
//!
//! real scalac prints `recv` once and scala-rs printed it **three** times.
//!
//! It also multiplies class files. Every lambda inside the receiver is emitted
//! again with each clone, so slick's
//! `Join(sj1, sj2, f1, f2.replace({ case … }), JoinType.Inner,
//! pred.replace({ case … })).infer()` produced three copies of both
//! `PartialFunction` literals: `.infer()` omits two defaults.
//!
//! The pass runs on the typed tree, before `uncurry`, so lambda-lift and the
//! capture analysis see the hoisted local like any other.
//!
//! Only a receiver that actually computes something is hoisted (an `Apply`,
//! `New`, `Block`, `If`, `Match`, `Try`). A path — `Ident`, `this`, `super`, a
//! literal, a field selection — is cheap to repeat and repeating it changes
//! nothing, so it is left where it is and no local is introduced.

use scala_rs_parser::{Flags, Modifiers, SymbolId, Tree, TreeKind};

use crate::lazy_local::children_mut;
use crate::symbol::{SymKind, SymbolTable};

/// Rewrite every call in `tree` that omitted defaults on a computed receiver.
pub fn hoist_default_receivers(tree: &mut Tree, st: &mut SymbolTable) {
    let mut p = Pass {
        st,
        gensym: 0,
        owner: SymbolId::NONE,
    };
    p.walk(tree);
}

struct Pass<'a> {
    st: &'a mut SymbolTable,
    gensym: u32,
    owner: SymbolId,
}

impl Pass<'_> {
    fn walk(&mut self, t: &mut Tree) {
        self.walk_at(t, true);
    }

    /// `outermost` is false for the `fun` of an enclosing application, i.e. for
    /// an inner clause of a curried call. Only the outermost `Apply` of a chain
    /// may be hoisted: wrapping `o.f(a)` of `o.f(a)(b)` in a block leaves
    /// `Apply { fun: Block, … }`, which has no callee symbol for the backend to
    /// emit (it came out as `throw new RuntimeException("unresolved apply")`,
    /// and is what stopped all twelve `slick_run.sh` programs at their first
    /// `.result`: `Invoker.foreach(f, maxRows = 0)(session)`).
    fn walk_at(&mut self, t: &mut Tree, outermost: bool) {
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
                self.walk_at(fun, false);
                for a in args.iter_mut() {
                    self.walk_at(a, true);
                }
            }
            _ => {
                for c in children_mut(t) {
                    self.walk_at(c, true);
                }
            }
        }
        self.owner = saved;
        if outermost {
            self.rewrite(t);
        }
    }

    fn rewrite(&mut self, t: &mut Tree) {
        let Some(prefix) = default_getter_prefix(t) else {
            return;
        };
        // The receiver sits at the innermost `Select` of a curried call
        // (`o.f(a)(b)` is nested `Apply`s until uncurry flattens them).
        if !receiver_needs_hoist(t) {
            return;
        }
        let ty = match innermost_qual(t) {
            Some(q) if !q.ty.is_no_type() && !q.ty.is_error() => q.ty.clone(),
            _ => return,
        };
        self.gensym += 1;
        let name = format!("qual$dflt${}", self.gensym);
        let tmp = self.st.alloc(
            name.clone(),
            self.owner,
            SymKind::Term,
            Flags::SYNTHETIC,
            "",
        );
        self.st.get_mut(tmp).ty = ty.clone();

        let mut ident = Tree::dummy(TreeKind::Ident { name: name.clone() });
        ident.sym = tmp;
        ident.ty = ty.clone();

        let Some(qual) = innermost_qual_mut(t) else {
            return;
        };
        let span = qual.span;
        ident.span = span;
        let recv = std::mem::replace(qual, ident.clone());

        // Every `name$default$n` argument this call carries was built from a
        // clone of that same receiver; point them at the local instead. The
        // defaults may sit in any clause of a curried call, not just the last.
        repoint_default_args(t, &prefix, &ident);

        let mut vd = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::SYNTHETIC),
            name,
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(recv),
        });
        vd.span = span;
        vd.sym = tmp;
        vd.ty = ty;

        let call = std::mem::replace(t, Tree::dummy(TreeKind::Empty));
        let call_ty = call.ty.clone();
        let call_span = call.span;
        *t = Tree {
            id: call.id,
            span: call_span,
            kind: TreeKind::Block {
                stats: vec![vd],
                expr: Box::new(call),
            },
            ty: call_ty,
            sym: SymbolId::NONE,
            postfix: false,
        };
    }
}

/// `"f$default$"` when `t` is an application of `f` carrying at least one
/// `f$default$n` argument built from the receiver. Any clause of a curried
/// call may carry one: `def foreach(f: R => Unit, maxRows: Int = 0)(implicit
/// session: Session)` called as `inv.foreach(x => …)(session)` has it in the
/// first, with a second clause applied after it.
fn default_getter_prefix(t: &Tree) -> Option<String> {
    let TreeKind::Apply { fun, .. } = &t.kind else {
        return None;
    };
    let name = callee_name(fun)?;
    let prefix = format!("{name}$default$");
    chain_has_default_arg(t, &prefix).then_some(prefix)
}

/// Whether any clause of the application chain at `t` passes a `prefix` getter.
fn chain_has_default_arg(t: &Tree, prefix: &str) -> bool {
    match &t.kind {
        TreeKind::Apply { fun, args } => {
            args.iter().any(|a| names_default_getter(a, prefix))
                || chain_has_default_arg(fun, prefix)
        }
        TreeKind::TypeApply { fun, .. } => chain_has_default_arg(fun, prefix),
        _ => false,
    }
}

/// Re-point every `prefix` getter argument in the chain at the hoisted local.
fn repoint_default_args(t: &mut Tree, prefix: &str, ident: &Tree) {
    match &mut t.kind {
        TreeKind::Apply { fun, args } => {
            for a in args.iter_mut() {
                if names_default_getter(a, prefix) {
                    if let Some(q) = innermost_qual_mut(a) {
                        *q = ident.clone();
                    }
                }
            }
            repoint_default_args(fun, prefix, ident);
        }
        TreeKind::TypeApply { fun, .. } => repoint_default_args(fun, prefix, ident),
        _ => {}
    }
}

/// The method name at the head of an application chain.
fn callee_name(t: &Tree) -> Option<&str> {
    match &t.kind {
        TreeKind::Select { name, .. } => Some(name),
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => callee_name(fun),
        _ => None,
    }
}

/// Whether `a` is a call to one of `prefix`'s getters *through a receiver*. A
/// default that could not be reached through a getter is inlined as its own
/// right-hand side and has no receiver to share.
fn names_default_getter(a: &Tree, prefix: &str) -> bool {
    callee_name(a).is_some_and(|n| n.starts_with(prefix)) && innermost_qual(a).is_some()
}

/// The qualifier of the `Select` at the head of an application chain.
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

fn receiver_needs_hoist(t: &Tree) -> bool {
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
    innermost_qual(t).is_some_and(computed)
}
