//! Distinct erasure phase: drop type arguments, erase type parameters to
//! `Object`, wrap by-name as `Function0`, and insert box/unbox trees so the
//! backend does not have to guess at call sites.

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use std::collections::HashSet;

use crate::symbol::SymbolTable;

/// Rewrite `tree` in place after typer, mutating symbol types to their JVM
/// (erased) forms.
pub fn erase(tree: &mut Tree, st: &mut SymbolTable) {
    erase_symbols(st);
    erase_tree(tree, st, None);
}

fn erase_symbols(st: &mut SymbolTable) {
    let n = st.symbols.len();
    for i in 1..n {
        let id = SymbolId(i as u32);
        let kind = st.get(id).kind;
        if matches!(
            kind,
            crate::symbol::SymKind::Class
                | crate::symbol::SymKind::Module
                | crate::symbol::SymKind::ModuleClass
                | crate::symbol::SymKind::Package
        ) {
            continue;
        }
        let ty = st.get(id).ty.clone();
        let erased = if kind == crate::symbol::SymKind::Method {
            erase_overriding_method(st, id, &ty)
        } else {
            erase_ty(&ty, st)
        };
        st.get_mut(id).ty = erased;
    }
}

fn erase_overriding_method(st: &SymbolTable, id: SymbolId, ty: &Type) -> Type {
    let erased = erase_ty(ty, st);
    let Some(ov) = find_overridden_method(st, id) else {
        return erased;
    };
    let ov_ret = match &st.get(ov).ty {
        Type::Method { ret, .. } | Type::Function { ret, .. } => erase_ty(ret, st),
        t => erase_ty(t, st),
    };
    let our_ret = match &erased {
        Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
        t => t.clone(),
    };
    if is_ref_erased(&ov_ret) && is_primitive(&our_ret) {
        match erased {
            Type::Method { paramss, .. } => Type::Method {
                paramss,
                ret: Box::new(ov_ret),
            },
            Type::Function { params, .. } => Type::Function {
                params,
                ret: Box::new(ov_ret),
            },
            other => other,
        }
    } else {
        erased
    }
}

fn find_overridden_method(st: &SymbolTable, id: SymbolId) -> Option<SymbolId> {
    let s = st.get(id);
    if s.kind != crate::symbol::SymKind::Method {
        return None;
    }
    let name = s.name.clone();
    let owner = s.owner;
    if owner.is_none() {
        return None;
    }
    let mut work = st.get(owner).parents.clone();
    let mut seen = HashSet::new();
    seen.insert(owner.0);
    while let Some(p) = work.pop() {
        let Some(pid) = st.class_sym_of(&p) else {
            continue;
        };
        if !seen.insert(pid.0) {
            continue;
        }
        for m in &st.get(pid).members {
            let mem = st.get(*m);
            if mem.kind == crate::symbol::SymKind::Method && mem.name == name && *m != id {
                return Some(*m);
            }
        }
        work.extend(st.get(pid).parents.clone());
    }
    None
}

pub fn erase_type(ty: &Type) -> Type {
    match ty {
        Type::TypeParam(_) | Type::TypeMember(_) => Type::Any,
        Type::Class { sym, .. } => Type::Class {
            sym: *sym,
            args: vec![],
        },
        Type::Named { name, args } if name == "Array" && args.len() == 1 => {
            let e = erase_type(&args[0]);
            if matches!(e, Type::Any | Type::AnyRef | Type::AnyVal) {
                Type::Any
            } else {
                Type::Array(Box::new(e))
            }
        }
        Type::Named { name, .. } => Type::Named {
            name: name.clone(),
            args: vec![],
        },
        Type::Array(t) => {
            let e = erase_type(t);
            if matches!(e, Type::Any | Type::AnyRef | Type::AnyVal) {
                Type::Any
            } else {
                Type::Array(Box::new(e))
            }
        }
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(erase_type).collect(),
            ret: Box::new(erase_type(ret)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(erase_type).collect())
                .collect(),
            ret: Box::new(erase_type(ret)),
        },
        Type::ByName(t) => Type::Function {
            params: vec![],
            ret: Box::new(erase_type(t)),
        },
        Type::Repeated(t) => Type::Repeated(Box::new(erase_type(t))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(erase_type).collect()),
        Type::Overload(alts) => Type::Overload(alts.iter().map(erase_type).collect()),
        Type::Wildcard | Type::BoundedWildcard { .. } => Type::Any,
        Type::Constant(lit) => Type::lit_underlying(lit),
        Type::ThisType(s) => Type::Class {
            sym: *s,
            args: vec![],
        },
        Type::SingleType { prefix, .. } => erase_type(prefix),
        Type::Annotated { tpe, .. } => erase_type(tpe),
        Type::Refined { parents, decls } => {
            if crate::symbol::SymbolTable::refined_has_term_members(decls) {
                Type::Refined {
                    parents: parents.iter().map(erase_type).collect(),
                    decls: decls.clone(),
                }
            } else if let Some(p) = parents.first() {
                erase_type(p)
            } else {
                Type::AnyRef
            }
        }
        other => other.clone(),
    }
}

fn erase_ty(ty: &Type, st: &SymbolTable) -> Type {
    match ty {
        Type::Class { sym, .. } if st.is_value_class(*sym) => {
            if let Some(u) = st.value_class_underlying(*sym) {
                erase_ty(&u, st)
            } else {
                Type::Any
            }
        }
        Type::TypeParam(_) | Type::TypeMember(_) => Type::Any,
        Type::Wildcard | Type::BoundedWildcard { .. } => Type::Any,
        Type::Constant(lit) => Type::lit_underlying(lit),
        Type::ThisType(s) => Type::Class {
            sym: *s,
            args: vec![],
        },
        Type::SingleType { prefix, sym } => {
            let t = st.get(*sym).ty.clone();
            if t.is_no_type() {
                erase_ty(prefix, st)
            } else {
                erase_ty(&t, st)
            }
        }
        Type::Annotated { tpe, .. } => erase_ty(tpe, st),
        Type::Refined { parents, decls } => {
            if crate::symbol::SymbolTable::refined_has_term_members(decls) {
                Type::Refined {
                    parents: parents.iter().map(|p| erase_ty(p, st)).collect(),
                    decls: decls.clone(),
                }
            } else if let Some(p) = parents.first() {
                erase_ty(p, st)
            } else {
                Type::AnyRef
            }
        }
        Type::Class { sym, .. } => Type::Class {
            sym: *sym,
            args: vec![],
        },
        Type::Named { name, args } if name == "Array" && args.len() == 1 => {
            let e = erase_ty(&args[0], st);
            if matches!(e, Type::Any | Type::AnyRef | Type::AnyVal) {
                Type::Any
            } else {
                Type::Array(Box::new(e))
            }
        }
        Type::Named { name, .. } => Type::Named {
            name: name.clone(),
            args: vec![],
        },
        Type::Array(t) => {
            let e = erase_ty(t, st);
            if matches!(e, Type::Any | Type::AnyRef | Type::AnyVal) {
                Type::Any
            } else {
                Type::Array(Box::new(e))
            }
        }
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| erase_ty(p, st)).collect(),
            ret: Box::new(erase_ty(ret, st)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| erase_ty(p, st)).collect())
                .collect(),
            ret: Box::new(erase_ty(ret, st)),
        },
        Type::ByName(t) => Type::Function {
            params: vec![],
            ret: Box::new(erase_ty(t, st)),
        },
        Type::Repeated(t) => Type::Repeated(Box::new(erase_ty(t, st))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|t| erase_ty(t, st)).collect()),
        Type::Overload(alts) => Type::Overload(alts.iter().map(|t| erase_ty(t, st)).collect()),
        other => other.clone(),
    }
}

fn is_primitive(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Long
        | Type::Double
        | Type::Boolean
        | Type::Char
        | Type::Float
        | Type::Unit => true,
        Type::Constant(lit) => is_primitive(&Type::lit_underlying(lit)),
        _ => false,
    }
}

fn is_ref_erased(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Any
            | Type::AnyRef
            | Type::AnyVal
            | Type::Class { .. }
            | Type::ModuleRef(_)
            | Type::String
            | Type::Array(_)
            | Type::Function { .. }
            | Type::TypeParam(_)
            | Type::TypeMember(_)
            | Type::Wildcard
            | Type::BoundedWildcard { .. }
            | Type::ThisType(_)
            | Type::SingleType { .. }
            | Type::Constant(_)
            | Type::Annotated { .. }
            | Type::Named { .. }
            | Type::Refined { .. }
    )
}

fn erase_tree(tree: &mut Tree, st: &SymbolTable, expected: Option<&Type>) {
    match &mut tree.kind {
        TreeKind::PackageDef { stats, .. } => {
            for s in stats {
                erase_tree(s, st, None);
            }
        }
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            for tp in tparams {
                erase_tree(tp, st, None);
            }
            for clause in vparamss {
                for p in clause {
                    erase_tree(p, st, None);
                }
            }
            for p in &mut impl_.parents {
                erase_tree(p, st, None);
            }
            for s in &mut impl_.body {
                erase_tree(s, st, None);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for p in &mut impl_.parents {
                erase_tree(p, st, None);
            }
            for s in &mut impl_.body {
                erase_tree(s, st, None);
            }
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            erase_tree(tpt, st, None);
            let pt = if tree.ty.is_no_type() {
                None
            } else {
                Some(erase_ty(&tree.ty, st))
            };
            erase_tree(rhs, st, pt.as_ref());
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            for tp in tparams {
                erase_tree(tp, st, None);
            }
            for clause in vparamss {
                for p in clause {
                    erase_tree(p, st, None);
                }
            }
            erase_tree(tpt, st, None);
            if !tree.sym.is_none() {
                let sty = st.get(tree.sym).ty.clone();
                if !matches!(sty, Type::NoType | Type::Error) {
                    tree.ty = sty;
                }
            }
            let ret = match &tree.ty {
                Type::Method { ret, .. } => Some(erase_ty(ret, st)),
                _ => None,
            };
            erase_tree(rhs, st, ret.as_ref());
        }
        TreeKind::TypeDef { rhs, .. } => {
            erase_tree(rhs, st, None);
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                erase_tree(s, st, None);
            }
            erase_tree(expr, st, expected);
        }
        TreeKind::If { cond, thenp, elsep } => {
            erase_tree(cond, st, Some(&Type::Boolean));
            erase_tree(thenp, st, expected);
            erase_tree(elsep, st, expected);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            erase_tree(cond, st, Some(&Type::Boolean));
            erase_tree(body, st, Some(&Type::Unit));
        }
        TreeKind::Assign { lhs, rhs } => {
            erase_tree(lhs, st, None);
            let pt = erase_ty(&lhs.ty, st);
            erase_tree(rhs, st, Some(&pt));
        }
        TreeKind::Match { selector, cases } => {
            erase_tree(selector, st, None);
            for c in cases {
                erase_tree(&mut c.pat, st, None);
                if !c.guard.is_empty() {
                    erase_tree(&mut c.guard, st, Some(&Type::Boolean));
                }
                erase_tree(&mut c.body, st, expected);
            }
        }
        TreeKind::Function { vparams, body } => {
            for p in vparams {
                erase_tree(p, st, None);
            }
            let ret = match &tree.ty {
                Type::Function { ret, .. } => Some(erase_ty(ret, st)),
                Type::Class { .. } => st.sam_sig(&tree.ty).map(|s| erase_ty(&s.ret_ty, st)),
                _ => None,
            };
            erase_tree(body, st, ret.as_ref());
        }
        TreeKind::Apply { .. } => {
            erase_apply(tree, st, expected);
            return;
        }
        TreeKind::TypeApply { fun, args } => {
            for a in args {
                erase_tree(a, st, None);
            }
            erase_tree(fun, st, None);
            tree.ty = fun.ty.clone();
        }
        TreeKind::Typed { expr, tpt } => {
            erase_tree(tpt, st, None);
            let pt = erase_ty(&tree.ty, st);
            erase_tree(expr, st, Some(&pt));
        }
        TreeKind::Select { qual, .. } => {
            erase_tree(qual, st, None);
            if !tree.sym.is_none() {
                let owner = st.get(tree.sym).owner;
                if st.is_value_class(owner)
                    && st.get(owner).ctor_fields.first().copied() == Some(tree.sym)
                {
                    let q = std::mem::replace(qual, Box::new(Tree::dummy(TreeKind::Empty)));
                    let mut inner = *q;
                    inner.ty = erase_ty(&inner.ty, st);
                    *tree = inner;
                    adapt_box_unbox(tree, expected);
                    return;
                }
                // Nullary methods (`it.next`, `opt.get`) stay as Select. If the
                // erased JVM return is `Object` and the specialized type is a
                // primitive, treat the Select as returning Object so the
                // backend does not `valueOf` an already-boxed value.
                if st.get(tree.sym).kind == crate::symbol::SymKind::Method {
                    let orig = tree.ty.clone();
                    let ret_erased = match &st.get(tree.sym).ty {
                        Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
                        t => erase_ty(t, st),
                    };
                    if is_primitive(&orig)
                        && is_ref_erased(&ret_erased)
                        && !matches!(orig, Type::Unit)
                    {
                        tree.ty = ret_erased;
                        wrap_unbox(tree, orig);
                        adapt_box_unbox(tree, expected);
                        return;
                    }
                    if matches!(orig, Type::String)
                        && is_ref_erased(&ret_erased)
                        && !matches!(ret_erased, Type::String)
                    {
                        tree.ty = ret_erased;
                        adapt_box_unbox(tree, expected);
                        return;
                    }
                }
            }
        }
        TreeKind::UnApply { fun, args } => {
            erase_tree(fun, st, None);
            for a in args {
                erase_tree(a, st, None);
            }
        }
        TreeKind::Super { .. } | TreeKind::This { .. } => {}
        TreeKind::New { tpt } => {
            erase_tree(tpt, st, None);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            erase_tree(expr, st, None);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            erase_tree(block, st, expected);
            for c in catches {
                erase_tree(&mut c.pat, st, None);
                erase_tree(&mut c.body, st, expected);
            }
            erase_tree(finalizer, st, Some(&Type::Unit));
        }
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                erase_tree(a, st, None);
            }
        }
        TreeKind::Ident { .. } => {
            erase_ident(tree, st);
        }
        _ => {}
    }

    let orig = tree.ty.clone();
    tree.ty = erase_ty(&orig, st);
    adapt_box_unbox(tree, expected);
}

fn erase_ident(tree: &mut Tree, st: &SymbolTable) {
    if tree.sym.is_none() {
        return;
    }
    let s = st.get(tree.sym);
    if s.flags.contains(Flags::BYNAME) {
        // `x` of type `=> T` becomes `x.apply()` after erasure to Function0.
        let span = tree.span;
        let inner_ty = match &tree.ty {
            Type::ByName(t) => erase_ty(t, st),
            Type::Function { ret, .. } => erase_ty(ret, st),
            t => erase_ty(t, st),
        };
        let mut fun = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        fun.ty = Type::Function {
            params: vec![],
            ret: Box::new(inner_ty.clone()),
        };
        *tree = Tree {
            id: fun.id,
            span,
            kind: TreeKind::Apply {
                fun: Box::new(fun),
                args: vec![],
            },
            ty: inner_ty,
            sym: SymbolId::NONE,
            postfix: false,
        };
    }
}

fn erase_apply(tree: &mut Tree, st: &SymbolTable, expected: Option<&Type>) {
    // `new Meter(x)` erases to `x` (the unique ctor arg) in the happy path.
    {
        let is_vc_new = match &tree.kind {
            TreeKind::Apply { fun, args } if !args.is_empty() => {
                if let TreeKind::New { .. } = &fun.kind {
                    let cid = if fun.sym.is_none() {
                        st.class_sym_of(&fun.ty)
                    } else {
                        Some(fun.sym)
                    }
                    .or_else(|| st.class_sym_of(&fun.ty));
                    cid.is_some_and(|c| st.is_value_class(c))
                } else {
                    false
                }
            }
            _ => false,
        };
        if is_vc_new {
            let mut arg = match &mut tree.kind {
                TreeKind::Apply { args, .. } => args.remove(0),
                _ => return,
            };
            let under = match &tree.ty {
                Type::Class { sym, .. } => {
                    st.value_class_underlying(*sym).unwrap_or_else(|| Type::Any)
                }
                t => t.clone(),
            };
            erase_tree(&mut arg, st, Some(&erase_ty(&under, st)));
            *tree = arg;
            tree.ty = erase_ty(&under, st);
            adapt_box_unbox(tree, expected);
            return;
        }
    }
    let param_tys;
    let mut fun_ty;
    {
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        erase_tree(fun, st, None);
        fun_ty = fun.ty.clone();
        param_tys = method_param_types(st, fun);
        if !fun.sym.is_none() {
            match &st.get(fun.sym).ty {
                Type::Method { ret, .. } | Type::Function { ret, .. } => {
                    fun_ty = Type::Method {
                        paramss: vec![param_tys.clone()],
                        ret: Box::new((**ret).clone()),
                    };
                }
                _ => {}
            }
        }
        for (i, a) in args.iter_mut().enumerate() {
            let p = param_tys.get(i).cloned();
            erase_tree(a, st, p.as_ref());
        }
    }
    let orig = tree.ty.clone();
    let orig_erased = erase_ty(&orig, st);
    let ret_erased = match &fun_ty {
        Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
        t => erase_ty(t, st),
    };
    tree.ty = orig_erased.clone();
    let array_prim_load = match &tree.kind {
        TreeKind::Apply { fun, .. } => match &fun.kind {
            TreeKind::Select { qual, name } if name == "apply" => match &qual.ty {
                Type::Array(elem) => is_primitive(elem),
                _ => false,
            },
            _ => false,
        },
        _ => false,
    };
    if is_primitive(&orig_erased)
        && is_ref_erased(&ret_erased)
        && !matches!(orig_erased, Type::Unit)
    {
        if array_prim_load {
            tree.ty = orig_erased;
        } else {
            tree.ty = ret_erased;
            wrap_unbox(tree, orig_erased);
        }
    } else if matches!(orig_erased, Type::String)
        && is_ref_erased(&ret_erased)
        && !matches!(ret_erased, Type::String)
    {
        tree.ty = ret_erased;
        wrap_unbox(tree, orig_erased);
    }
    adapt_box_unbox(tree, expected);
}

fn method_param_types(st: &SymbolTable, fun: &Tree) -> Vec<Type> {
    if matches!(&fun.kind, TreeKind::New { .. }) {
        let cid = if fun.sym.is_none() {
            st.class_sym_of(&fun.ty)
        } else {
            Some(fun.sym)
        }
        .or_else(|| st.class_sym_of(&fun.ty));
        if let Some(c) = cid {
            for m in &st.get(c).members {
                if st.get(*m).name == "<init>" {
                    if let Type::Method { paramss, .. } = &st.get(*m).ty {
                        return paramss.iter().flatten().cloned().collect();
                    }
                }
            }
            return st
                .get(c)
                .ctor_fields
                .iter()
                .map(|f| erase_ty(&st.get(*f).ty, st))
                .collect();
        }
    }
    if !fun.sym.is_none() {
        let owner = st.get(fun.sym).owner;
        if owner == st.array_sym {
            match &fun.ty {
                Type::Method { paramss, .. } => {
                    return paramss.iter().flatten().map(|p| erase_ty(p, st)).collect();
                }
                Type::Function { params, .. } => {
                    return params.iter().map(|p| erase_ty(p, st)).collect();
                }
                _ => {}
            }
        }
        match &st.get(fun.sym).ty {
            Type::Method { paramss, .. } => {
                return paramss.iter().flatten().cloned().collect();
            }
            Type::Function { params, .. } => return params.clone(),
            _ => {}
        }
    }
    match &fun.ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        Type::Function { params, .. } => params.clone(),
        _ => Vec::new(),
    }
}

fn adapt_box_unbox(tree: &mut Tree, expected: Option<&Type>) {
    let Some(exp) = expected else {
        return;
    };
    let got = &tree.ty;
    if is_primitive(got) && is_ref_erased(exp) && !matches!(exp, Type::Unit) {
        wrap_box(tree);
        return;
    }
    if is_ref_erased(got) && is_primitive(exp) && !matches!(exp, Type::Unit) {
        wrap_unbox(tree, exp.clone());
    }
}

fn wrap_box(tree: &mut Tree) {
    let span = tree.span;
    let orig_ty = tree.ty.clone();
    let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    let fun = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Ident {
            name: "$box".into(),
        },
        ty: Type::Method {
            paramss: vec![vec![orig_ty.clone()]],
            ret: Box::new(Type::Any),
        },
        sym: SymbolId::NONE,
        postfix: false,
    };
    *tree = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Apply {
            fun: Box::new(fun),
            args: vec![inner],
        },
        ty: Type::Any,
        sym: SymbolId::NONE,
        postfix: false,
    };
}

fn wrap_unbox(tree: &mut Tree, to: Type) {
    let span = tree.span;
    let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    let fun = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Ident {
            name: "$unbox".into(),
        },
        ty: Type::Method {
            paramss: vec![vec![Type::Any]],
            ret: Box::new(to.clone()),
        },
        sym: SymbolId::NONE,
        postfix: false,
    };
    *tree = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Apply {
            fun: Box::new(fun),
            args: vec![inner],
        },
        ty: to,
        sym: SymbolId::NONE,
        postfix: false,
    };
}
