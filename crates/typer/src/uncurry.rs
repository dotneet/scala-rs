//! Distinct uncurry phase (nsc-style), between typer and erasure.
//!
//! Nested parameter lists become one list. Nested `Apply` trees become a
//! single apply. Leftover method types used as values (eta-expansion and
//! partial application) become `FunctionN` closures.

use scala_rs_parser::{Flags, Modifiers, SymbolId, Tree, TreeKind, Type};

use crate::symbol::{SymKind, SymbolTable};

/// Rewrite `tree` in place after typer: flatten curried methods / applies and
/// eta-expand remaining method values.
pub fn uncurry(tree: &mut Tree, st: &mut SymbolTable) {
    let mut u = Uncurry { st, gensym: 0 };
    u.walk(tree, false);
    flatten_method_symbols(u.st);
}

struct Uncurry<'a> {
    st: &'a mut SymbolTable,
    gensym: u32,
}

impl<'a> Uncurry<'a> {
    fn walk(&mut self, tree: &mut Tree, as_fun: bool) {
        match &mut tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.walk(s, false);
                }
            }
            TreeKind::ClassDef {
                tparams,
                vparamss,
                impl_,
                ..
            } => {
                for tp in tparams {
                    self.walk(tp, false);
                }
                for clause in vparamss {
                    for p in clause {
                        self.walk(p, false);
                    }
                }
                for p in &mut impl_.parents {
                    self.walk(p, false);
                }
                for s in &mut impl_.body {
                    self.walk(s, false);
                }
            }
            TreeKind::ModuleDef { impl_, .. } => {
                for p in &mut impl_.parents {
                    self.walk(p, false);
                }
                for s in &mut impl_.body {
                    self.walk(s, false);
                }
            }
            TreeKind::ValDef { tpt, rhs, .. } => {
                self.walk(tpt, false);
                self.walk(rhs, false);
            }
            TreeKind::DefDef { .. } => {
                if let TreeKind::DefDef {
                    tparams,
                    vparamss,
                    tpt,
                    rhs,
                    ..
                } = &mut tree.kind
                {
                    for tp in tparams {
                        self.walk(tp, false);
                    }
                    for clause in vparamss {
                        for p in clause {
                            self.walk(p, false);
                        }
                    }
                    self.walk(tpt, false);
                    self.walk(rhs, false);
                }
                flatten_defdef(tree, self.st);
            }
            TreeKind::TypeDef { rhs, .. } => self.walk(rhs, false),
            TreeKind::Block { stats, expr } => {
                for s in stats {
                    self.walk(s, false);
                }
                self.walk(expr, false);
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.walk(cond, false);
                self.walk(thenp, false);
                self.walk(elsep, false);
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                self.walk(cond, false);
                self.walk(body, false);
            }
            TreeKind::Assign { lhs, rhs } => {
                self.walk(lhs, false);
                self.walk(rhs, false);
            }
            TreeKind::Match { selector, cases } => {
                self.walk(selector, false);
                for c in cases {
                    self.walk(&mut c.pat, false);
                    if !c.guard.is_empty() {
                        self.walk(&mut c.guard, false);
                    }
                    self.walk(&mut c.body, false);
                }
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.walk(p, false);
                }
                self.walk(body, false);
            }
            TreeKind::Apply { .. } => {
                if let TreeKind::Apply { fun, args } = &mut tree.kind {
                    self.walk(fun, true);
                    for a in args {
                        self.walk(a, false);
                    }
                }
                flatten_apply(tree);
                if !as_fun {
                    self.eta_if_method(tree);
                }
            }
            TreeKind::TypeApply { .. } => {
                if let TreeKind::TypeApply { fun, args } = &mut tree.kind {
                    for a in args {
                        self.walk(a, false);
                    }
                    self.walk(fun, as_fun);
                }
                if !as_fun {
                    self.eta_if_method(tree);
                }
            }
            TreeKind::Typed { .. } => {
                let eta = if let TreeKind::Typed { tpt, .. } = &tree.kind {
                    is_eta_marker(tpt)
                } else {
                    false
                };
                if let TreeKind::Typed { expr, tpt } = &mut tree.kind {
                    self.walk(tpt, false);
                    self.walk(expr, as_fun);
                }
                if eta {
                    if let TreeKind::Typed { expr, .. } = &mut tree.kind {
                        let mut inner =
                            std::mem::replace(expr, Box::new(Tree::dummy(TreeKind::Empty)));
                        inner.ty = tree.ty.clone();
                        inner.sym = tree.sym;
                        *tree = *inner;
                    }
                    self.eta_if_method(tree);
                } else if !as_fun {
                    self.eta_if_method(tree);
                }
            }
            TreeKind::Select { .. } => {
                if let TreeKind::Select { qual, .. } = &mut tree.kind {
                    self.walk(qual, false);
                }
                if !as_fun {
                    self.eta_if_method(tree);
                }
            }
            TreeKind::Ident { .. } => {
                if !as_fun {
                    self.eta_if_method(tree);
                }
            }
            TreeKind::New { tpt } => self.walk(tpt, false),
            TreeKind::Return { expr } | TreeKind::Throw { expr } => self.walk(expr, false),
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                self.walk(block, false);
                for c in catches {
                    self.walk(&mut c.pat, false);
                    self.walk(&mut c.body, false);
                }
                self.walk(finalizer, false);
            }
            TreeKind::InterpolatedString { args, .. } => {
                for a in args {
                    self.walk(a, false);
                }
            }
            TreeKind::UnApply { fun, args } => {
                self.walk(fun, true);
                for a in args {
                    self.walk(a, false);
                }
            }
            TreeKind::LabelDef { params, rhs, .. } => {
                for p in params {
                    self.walk(p, false);
                }
                self.walk(rhs, false);
            }
            TreeKind::Bind { body, .. } => self.walk(body, false),
            TreeKind::Star { elem } => self.walk(elem, false),
            TreeKind::Alternative { trees } => {
                for t in trees {
                    self.walk(t, false);
                }
            }
            TreeKind::AppliedTypeTree { tpt, args } => {
                self.walk(tpt, false);
                for a in args {
                    self.walk(a, false);
                }
            }
            _ => {}
        }
    }

    fn eta_if_method(&mut self, tree: &mut Tree) {
        if matches!(&tree.kind, TreeKind::Function { .. }) {
            return;
        }
        let (paramss, ret) = match &tree.ty {
            Type::Method { paramss, ret } => (paramss.clone(), (**ret).clone()),
            _ => return,
        };
        let params: Vec<Type> = paramss.into_iter().flatten().collect();
        eta_expand(self.st, &mut self.gensym, tree, params, ret);
    }
}

pub(crate) fn is_eta_marker(tpt: &Tree) -> bool {
    matches!(
        &tpt.kind,
        TreeKind::Function { vparams, body } if vparams.is_empty() && body.is_empty()
    )
}

pub(crate) fn eta_expand(
    st: &mut SymbolTable,
    gensym: &mut u32,
    tree: &mut Tree,
    params: Vec<Type>,
    ret: Type,
) {
    let span = tree.span;
    let mut vparams = Vec::new();
    let mut args = Vec::new();
    for pty in &params {
        *gensym += 1;
        let name = format!("x$eta${}", *gensym);
        let id = st.alloc(&name, st.owner, SymKind::Term, Flags::PARAM, "");
        st.get_mut(id).ty = pty.clone();
        let mut vd = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::PARAM),
            name: name.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(Tree::dummy(TreeKind::Empty)),
        });
        vd.span = span;
        vd.sym = id;
        vd.ty = pty.clone();
        vparams.push(vd);
        let mut ident = Tree::dummy(TreeKind::Ident { name });
        ident.span = span;
        ident.sym = id;
        ident.ty = pty.clone();
        args.push(ident);
    }
    let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    let fun_sym = inner.sym;
    let apply = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Apply {
            fun: Box::new(inner),
            args,
        },
        ty: ret.clone(),
        sym: fun_sym,
        postfix: false,
    };
    *tree = Tree {
        id: apply.id,
        span,
        kind: TreeKind::Function {
            vparams,
            body: Box::new(apply),
        },
        ty: Type::Function {
            params,
            ret: Box::new(ret),
        },
        sym: SymbolId::NONE,
        postfix: false,
    };
}

fn flatten_apply(tree: &mut Tree) {
    loop {
        match &tree.kind {
            TreeKind::Apply { fun, .. } => {
                if !matches!(&fun.kind, TreeKind::Apply { .. }) {
                    return;
                }
                if matches!(&peel_new(fun).kind, TreeKind::New { .. }) {
                    return;
                }
                // Only a curried *method*'s clauses collapse into one call: a
                // partial application leaves a method type behind. An inner
                // application whose *result* is a function value
                // (`f.curried(3)(4)`, `Function.untupled(g)(1, 2)`) is a call
                // in its own right, and merging the argument lists would push
                // the outer ones onto the inner `apply`.
                if matches!(fun.ty, Type::Function { .. }) {
                    return;
                }
            }
            _ => return,
        }
        let span = tree.span;
        let ty = tree.ty.clone();
        let sym = tree.sym;
        let id = tree.id;
        let TreeKind::Apply { fun, args } = std::mem::replace(&mut tree.kind, TreeKind::Empty)
        else {
            return;
        };
        match fun.kind {
            TreeKind::Apply {
                fun: inner,
                args: mut ia,
            } => {
                ia.extend(args);
                *tree = Tree {
                    id,
                    span,
                    kind: TreeKind::Apply {
                        fun: inner,
                        args: ia,
                    },
                    ty,
                    sym,
                    postfix: false,
                };
            }
            other => {
                *tree = Tree {
                    id,
                    span,
                    kind: TreeKind::Apply {
                        fun: Box::new(Tree::dummy(other)),
                        args,
                    },
                    ty,
                    sym,
                    postfix: false,
                };
                return;
            }
        }
    }
}

fn peel_new(tree: &Tree) -> &Tree {
    match &tree.kind {
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => peel_new(fun),
        _ => tree,
    }
}

fn flatten_defdef(tree: &mut Tree, st: &mut SymbolTable) {
    let TreeKind::DefDef { vparamss, .. } = &mut tree.kind else {
        return;
    };
    if vparamss.len() <= 1 {
        return;
    }
    let flat: Vec<Tree> = vparamss.drain(..).flatten().collect();
    *vparamss = vec![flat];
    if let Type::Method { paramss, ret } = &tree.ty {
        if paramss.len() > 1 {
            let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
            tree.ty = Type::Method {
                paramss: vec![params],
                ret: ret.clone(),
            };
        }
    }
    if !tree.sym.is_none() {
        flatten_one_method(st, tree.sym);
        st.get_mut(tree.sym).ty = tree.ty.clone();
    }
}

fn flatten_one_method(st: &mut SymbolTable, id: SymbolId) {
    let paramss = st.get(id).paramss.clone();
    if paramss.len() > 1 {
        let flat: Vec<SymbolId> = paramss.into_iter().flatten().collect();
        st.get_mut(id).params = flat.clone();
        st.get_mut(id).paramss = vec![flat];
    }
    if let Type::Method { paramss, ret } = st.get(id).ty.clone() {
        if paramss.len() > 1 {
            let params: Vec<Type> = paramss.into_iter().flatten().collect();
            st.get_mut(id).ty = Type::Method {
                paramss: vec![params],
                ret,
            };
        }
    }
}

fn flatten_method_symbols(st: &mut SymbolTable) {
    let n = st.symbols.len();
    for i in 1..n {
        let id = SymbolId(i as u32);
        if st.get(id).kind == SymKind::Method {
            flatten_one_method(st, id);
        }
    }
}

/// Eta-expand a method with several parameter lists into nested lambdas:
/// `curry _` on `def curry(a: Int)(b: Int)(c: Int): Int` is
/// `Int => Int => Int => Int`, one lambda per list, each applying its own
/// parameters to the partial application built so far.
pub(crate) fn eta_expand_curried(
    st: &mut SymbolTable,
    gensym: &mut u32,
    tree: &mut Tree,
    paramss: &[Vec<Type>],
    ret: Type,
) {
    let Some((first, rest)) = paramss.split_first() else {
        return;
    };
    if rest.is_empty() {
        eta_expand(st, gensym, tree, first.clone(), ret);
        return;
    }
    let span = tree.span;
    let mut vparams = Vec::new();
    let mut args = Vec::new();
    for pty in first {
        *gensym += 1;
        let name = format!("x$eta${}", *gensym);
        let id = st.alloc(&name, st.owner, SymKind::Term, Flags::PARAM, "");
        st.get_mut(id).ty = pty.clone();
        let mut vd = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::PARAM),
            name: name.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(Tree::dummy(TreeKind::Empty)),
        });
        vd.span = span;
        vd.sym = id;
        vd.ty = pty.clone();
        vparams.push(vd);
        let mut ident = Tree::dummy(TreeKind::Ident { name });
        ident.span = span;
        ident.sym = id;
        ident.ty = pty.clone();
        args.push(ident);
    }
    let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    let fun_sym = inner.sym;
    let mut body = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Apply {
            fun: Box::new(inner),
            args,
        },
        ty: Type::NoType,
        sym: fun_sym,
        postfix: false,
    };
    eta_expand_curried(st, gensym, &mut body, rest, ret);
    let body_ty = body.ty.clone();
    *tree = Tree {
        id: body.id,
        span,
        kind: TreeKind::Function {
            vparams,
            body: Box::new(body),
        },
        ty: Type::Function {
            params: first.clone(),
            ret: Box::new(body_ty),
        },
        sym: SymbolId::NONE,
        postfix: false,
    };
}
