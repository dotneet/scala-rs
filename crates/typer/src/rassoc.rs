//! Evaluate the left operand of a right-associative operator first.
//!
//! `a :: b` calls `b.::(a)`, but Scala evaluates the operands left to right:
//! nsc's parser binds the left operand to a local before the call,
//!
//! ```scala
//! e(1) :: e(2) :: Nil
//! // becomes
//! { val rassoc$1 = e(1); { val rassoc$2 = e(2); Nil.::(rassoc$2) }.::(rassoc$1) }
//! ```
//!
//! and its typer puts the operand back in place only when that cannot be
//! observed -- a pure expression, or a by-name parameter (`x #:: rest` on a
//! `LazyList` must not force `x`). scala-rs's parser builds the call directly,
//! so the receiver ran first: `readA() :: readB() :: Nil` read `B` before
//! `A`, and `e(1) :: e(2) :: e(Nil)` logged `List() 2 1` where scalac logs
//! `1 2 List()`.
//!
//! This pass restores the order on the typed tree, where both halves of
//! nsc's decision are known: whether the parameter is by-name (the argument
//! is then a `byname_thunk`) and whether either operand computes anything.
//! It runs before `uncurry`, next to `hoist_default_receivers`, so lambda-lift
//! and the capture analysis see the local like any other.
//!
//! Only infix syntax is reordered. A written `b.::(a)` is an ordinary call
//! whose receiver really is evaluated first; the parser gives the infix form's
//! `Select` the operator's own position, which starts *before* its receiver.

use scala_rs_parser::{Flags, Modifiers, SymbolId, Tree, TreeKind, Type};

use crate::lazy_local::children_mut;
use crate::symbol::{SymKind, SymbolTable};

/// Rewrite every infix right-associative application in `tree` whose left
/// operand and receiver both compute something.
pub fn restore_rassoc_order(tree: &mut Tree, st: &mut SymbolTable) {
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

    /// `outermost` is false for the `fun` of an enclosing application: a
    /// right-associative operator with a second (implicit) clause is
    /// `Apply(Apply(Select(recv, op), [arg]), [ev])`, and only the whole
    /// chain may be wrapped in the block -- `Apply { fun: Block }` has no
    /// callee for the backend (`hoist_default_receivers` learned the same).
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
        let Some(app) = operator_apply(t) else {
            return;
        };
        let TreeKind::Apply { fun, args } = &app.kind else {
            return;
        };
        // The operand is the first clause's only argument. An implicit clause
        // is appended to the same argument list by the typer
        // (`def ::(x: Int)(implicit t: T)` gives `[x, t]`).
        let op = fun_sym(fun);
        let one_param =
            !op.is_none() && self.st.get(op).paramss.first().map(|p| p.len()) == Some(1);
        if args.is_empty() || (args.len() != 1 && !one_param) || !is_infix_rassoc(fun) {
            return;
        }
        let arg = &args[0];
        // A by-name parameter keeps its operand unevaluated, as nsc's typer
        // does when it inlines the `rassoc$` local back into the call.
        if arg.byname_thunk || !computes(self.st, arg) || !receiver_computes(self.st, fun) {
            return;
        }
        let ty = arg.ty.clone();
        if ty.is_no_type() || ty.is_error() {
            return;
        }
        self.gensym += 1;
        let name = format!("rassoc${}", self.gensym);
        let tmp = self.st.alloc(
            name.clone(),
            self.owner,
            SymKind::Term,
            Flags::SYNTHETIC,
            "",
        );
        self.st.get_mut(tmp).ty = ty.clone();

        let Some(app) = operator_apply_mut(t) else {
            return;
        };
        let TreeKind::Apply { args, .. } = &mut app.kind else {
            return;
        };
        let span = args[0].span;
        let mut ident = Tree::dummy(TreeKind::Ident { name: name.clone() });
        ident.sym = tmp;
        ident.ty = ty.clone();
        ident.span = span;
        let operand = std::mem::replace(&mut args[0], ident);

        let mut vd = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::SYNTHETIC),
            name,
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(operand),
        });
        vd.span = span;
        vd.sym = tmp;
        vd.ty = ty;

        let call = std::mem::replace(t, Tree::dummy(TreeKind::Empty));
        let mut block = Tree::dummy(TreeKind::Block {
            stats: vec![vd],
            expr: Box::new(Tree::dummy(TreeKind::Empty)),
        });
        block.id = call.id;
        block.span = call.span;
        block.ty = call.ty.clone();
        if let TreeKind::Block { expr, .. } = &mut block.kind {
            **expr = call;
        }
        *t = block;
    }
}

/// The application of the operator itself at the head of `t`'s chain: `t`,
/// or the innermost `Apply` under further argument clauses.
fn operator_apply(t: &Tree) -> Option<&Tree> {
    let TreeKind::Apply { fun, .. } = &t.kind else {
        return None;
    };
    match &fun.kind {
        TreeKind::Apply { .. } => operator_apply(fun),
        _ => Some(t),
    }
}

fn operator_apply_mut(t: &mut Tree) -> Option<&mut Tree> {
    let TreeKind::Apply { fun, .. } = &t.kind else {
        return None;
    };
    if matches!(fun.kind, TreeKind::Apply { .. }) {
        let TreeKind::Apply { fun, .. } = &mut t.kind else {
            return None;
        };
        return operator_apply_mut(fun);
    }
    Some(t)
}

/// `recv op arg` written infix, `op` right-associative: the callee is a
/// `Select` (possibly under a `TypeApply`) whose span starts at the operator,
/// before the receiver it selects from.
fn is_infix_rassoc(fun: &Tree) -> bool {
    let sel = match &fun.kind {
        TreeKind::TypeApply { fun, .. } => fun.as_ref(),
        _ => fun,
    };
    let TreeKind::Select { qual, name } = &sel.kind else {
        return false;
    };
    name.len() > 1
        && name.ends_with(':')
        && sel.span.lo < qual.span.lo
        && !sel.span.is_dummy()
        && !qual.span.is_dummy()
}

fn fun_sym(fun: &Tree) -> SymbolId {
    match &fun.kind {
        TreeKind::TypeApply { fun, .. } => fun.sym,
        _ => fun.sym,
    }
}

fn receiver_computes(st: &SymbolTable, fun: &Tree) -> bool {
    match &fun.kind {
        TreeKind::TypeApply { fun, .. } => receiver_computes(st, fun),
        TreeKind::Select { qual, .. } => computes(st, qual),
        _ => false,
    }
}

/// Whether evaluating `t` can run code: a call, an allocation, a block or a
/// branching expression. A path, a literal or `this` reads the same whenever
/// it is evaluated, so it needs no local.
fn computes(st: &SymbolTable, t: &Tree) -> bool {
    // A paren-less method call (`it.next`, a local `def f`) is a bare
    // `Select` / `Ident` and runs code like any other call, and so does a
    // by-name parameter: `x :: x :: xs` forces `x` twice, left one first.
    if !t.sym.is_none()
        && matches!(t.kind, TreeKind::Select { .. } | TreeKind::Ident { .. })
        && (st.get(t.sym).kind == SymKind::Method
            || matches!(st.get(t.sym).ty, Type::ByName(_))
            || matches!(t.ty, Type::ByName(_)))
    {
        return true;
    }
    match &t.kind {
        TreeKind::Apply { .. }
        | TreeKind::New { .. }
        | TreeKind::Block { .. }
        | TreeKind::If { .. }
        | TreeKind::Match { .. }
        | TreeKind::Try { .. }
        | TreeKind::Assign { .. }
        | TreeKind::Throw { .. } => true,
        TreeKind::Typed { expr, .. } => computes(st, expr),
        TreeKind::TypeApply { fun, .. } => computes(st, fun),
        TreeKind::Select { qual, .. } => computes(st, qual),
        _ => false,
    }
}
