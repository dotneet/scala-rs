//! Distinct erasure phase: drop type arguments, erase type parameters to
//! `Object`, wrap by-name as `Function0`, and insert box/unbox trees so the
//! backend does not have to guess at call sites.

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};

use crate::symbol::{Intrinsic, SymbolTable};

/// Rewrite `tree` in place after typer, mutating symbol types to their JVM
/// (erased) forms.
pub fn erase(tree: &mut Tree, st: &mut SymbolTable) {
    let boxed_params = value_class_lambda_params(tree, st);
    erase_symbols(st);
    // A `FunctionN.apply` takes `Object`, so a lambda parameter instantiated at
    // a value class receives the *boxed* instance: `xs.map(_.n)` over a
    // `List[Meters]` is handed a `Meters`, not an `Integer`.
    for (p, c) in &boxed_params {
        st.get_mut(*p).ty = Type::Class {
            sym: *c,
            args: vec![],
        };
    }
    // These writes are the one thing that can make the next unit's
    // `erase_symbols` do work again; see `SymbolTable::erasure_settled`.
    if !boxed_params.is_empty() {
        st.erasure_settled = false;
    }
    erase_tree(tree, st, None);
}

fn value_class_lambda_params(tree: &Tree, st: &SymbolTable) -> Vec<(SymbolId, SymbolId)> {
    let mut out = Vec::new();
    collect_value_class_lambda_params(tree, st, &mut out);
    out
}

fn collect_value_class_lambda_params(
    tree: &Tree,
    st: &SymbolTable,
    out: &mut Vec<(SymbolId, SymbolId)>,
) {
    if let TreeKind::Function { vparams, .. } = &tree.kind {
        // A SAM instance keeps the interface's own erased signature, which may
        // already take the underlying value; only the `FunctionN` shape is
        // guaranteed to hand over `Object`.
        if matches!(tree.ty, Type::Function { .. }) {
            for p in vparams {
                if p.sym.is_none() {
                    continue;
                }
                let ty = if p.ty.is_no_type() {
                    st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                if let Some(c) = value_class_of(&ty, st) {
                    out.push((p.sym, c));
                }
            }
        }
    }
    for_each_child(tree, &mut |c| collect_value_class_lambda_params(c, st, out));
}

/// Every subtree that can hold a term. Only used by passes that look for one
/// specific shape, so type-only children are visited too rather than listed
/// separately.
fn for_each_child(tree: &Tree, f: &mut impl FnMut(&Tree)) {
    match &tree.kind {
        TreeKind::PackageDef { stats, .. } => {
            for s in stats {
                f(s);
            }
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                f(s);
            }
            f(expr);
        }
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            for t in tparams {
                f(t);
            }
            for clause in vparamss {
                for p in clause {
                    f(p);
                }
            }
            for p in &impl_.parents {
                f(p);
            }
            for s in &impl_.body {
                f(s);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for p in &impl_.parents {
                f(p);
            }
            for s in &impl_.body {
                f(s);
            }
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            f(tpt);
            f(rhs);
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            for t in tparams {
                f(t);
            }
            for clause in vparamss {
                for p in clause {
                    f(p);
                }
            }
            f(tpt);
            f(rhs);
        }
        TreeKind::TypeDef { rhs, .. } => f(rhs),
        TreeKind::LabelDef { rhs, .. } => f(rhs),
        TreeKind::If { cond, thenp, elsep } => {
            f(cond);
            f(thenp);
            f(elsep);
        }
        TreeKind::Match { selector, cases } => {
            f(selector);
            for c in cases {
                f(&c.pat);
                f(&c.guard);
                f(&c.body);
            }
        }
        TreeKind::Function { vparams, body } => {
            for p in vparams {
                f(p);
            }
            f(body);
        }
        TreeKind::Assign { lhs, rhs } => {
            f(lhs);
            f(rhs);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            f(cond);
            f(body);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            f(expr)
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            f(block);
            for c in catches {
                f(&c.pat);
                f(&c.guard);
                f(&c.body);
            }
            f(finalizer);
        }
        TreeKind::Typed { expr, tpt } => {
            f(expr);
            f(tpt);
        }
        TreeKind::TypeApply { fun, args } | TreeKind::Apply { fun, args } => {
            f(fun);
            for a in args {
                f(a);
            }
        }
        TreeKind::UnApply { fun, args } => {
            f(fun);
            for a in args {
                f(a);
            }
        }
        TreeKind::Select { qual, .. }
        | TreeKind::SelectFromTypeTree { qual, .. }
        | TreeKind::Bind { body: qual, .. }
        | TreeKind::Star { elem: qual }
        | TreeKind::SingletonTypeTree { ref_: qual } => f(qual),
        TreeKind::Alternative { trees } => {
            for t in trees {
                f(t);
            }
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            f(tpt);
            for a in args {
                f(a);
            }
        }
        TreeKind::AnnotatedTypeTree { tpt, .. } => f(tpt),
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                f(a);
            }
        }
        _ => {}
    }
}

/// Erase every symbol's type in place.
///
/// `erase` runs once per compilation unit and this pass is over the *whole*
/// symbol table, so with slick (184 units, ~10^5 symbols, most of them read out
/// of jars) it was 55% of the entire compile — quadratic in the number of
/// files. It is a fixpoint loop, though, and one that usually converges after
/// two rounds: a pass that changes nothing leaves `st` exactly as it found it,
/// so the next pass over the same table cannot change anything either. That is
/// what `erasure_settled` records. The only writes to a symbol's type between
/// two passes are the boxed lambda parameters in `erase` above, which clear the
/// flag, and `alloc`, which clears it too — `erase_tree` writes tree types
/// only. Skipping a settled pass is therefore not an approximation: the pass
/// that is skipped provably had no effect.
fn erase_symbols(st: &mut SymbolTable) {
    if st.erasure_settled {
        return;
    }
    let mut changed = false;
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
        // Borrowed, not cloned: `erase_ty` and `value_class_of` only read, and
        // deep-cloning every symbol's type was a tenth of the whole compile.
        let (erased, value_class) = {
            let st: &SymbolTable = st;
            let ty = &st.get(id).ty;
            let value_class = if kind == crate::symbol::SymKind::Term {
                value_class_of(ty, st)
            } else {
                None
            };
            let erased = if kind == crate::symbol::SymKind::Method {
                erase_overriding_method(st, id, ty)
            } else {
                erase_ty(ty, st)
            };
            (erased, value_class)
        };
        if let Some(c) = value_class {
            st.value_class_terms.insert(id, c);
        }
        if st.get(id).ty != erased {
            changed = true;
            st.get_mut(id).ty = erased;
        }
    }
    st.erasure_settled = !changed;
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
    let name = &s.name;
    let owner = s.owner;
    if owner.is_none() {
        return None;
    }
    // A worklist of symbols, not of parent *types*: the walk only ever asks a
    // parent for its class, and this runs once per method symbol in the table,
    // so cloning each node's parent list -- a deep copy of every type in it --
    // was most of what erasure allocated. `seen` is a scanned `Vec` for the
    // same reason: a hierarchy is tens of nodes.
    let mut seen: Vec<u32> = vec![owner.0];
    let mut work: Vec<SymbolId> = Vec::new();
    let push_parents = |work: &mut Vec<SymbolId>, of: SymbolId| {
        for p in &st.get(of).parents {
            if let Some(q) = st.class_sym_of(p) {
                work.push(q);
            }
        }
    };
    push_parents(&mut work, owner);
    while let Some(pid) = work.pop() {
        if seen.contains(&pid.0) {
            continue;
        }
        seen.push(pid.0);
        for m in &st.get(pid).members {
            let mem = st.get(*m);
            if mem.kind == crate::symbol::SymKind::Method && mem.name == *name && *m != id {
                return Some(*m);
            }
        }
        push_parents(&mut work, pid);
    }
    None
}

/// nsc erases `Array[T]` to `Object` only when the element is an *abstract*
/// type: `def d[T](x: Array[T])` takes a plain `Object`. A concrete element
/// that merely erases to `Object` keeps the array -- `Array[AnyRef]`,
/// `Array[Any]` and `Array[AnyVal]` are all `[Ljava/lang/Object;` in nsc.
fn array_elem_is_abstract(elem: &Type) -> bool {
    match elem {
        Type::TypeParam(_)
        | Type::TypeMember(_)
        | Type::Applied { .. }
        | Type::Wildcard
        | Type::BoundedWildcard { .. } => true,
        Type::Annotated { tpe, .. } => array_elem_is_abstract(tpe),
        _ => false,
    }
}

pub fn erase_type(ty: &Type) -> Type {
    match ty {
        Type::TypeParam(_) | Type::TypeMember(_) => Type::Any,
        Type::Applied { .. } => Type::Any,
        Type::Class { sym, .. } => Type::Class {
            sym: *sym,
            args: vec![],
        },
        Type::Named { name, args } if name == "Array" && args.len() == 1 => {
            let e = erase_type(&args[0]);
            if array_elem_is_abstract(&args[0])
                && matches!(e, Type::Any | Type::AnyRef | Type::AnyVal)
            {
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
            if array_elem_is_abstract(t) && matches!(e, Type::Any | Type::AnyRef | Type::AnyVal) {
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

/// The erasure of an *element* type -- array element, repeated parameter. A
/// value class is boxed there: nsc erases `Array[Meters]` to `[LMeters;`, not
/// to `[I`, so `arr.mkString` prints `Meters@1` and not `1`.
fn erase_elem_ty(ty: &Type, st: &SymbolTable) -> Type {
    match value_class_of(ty, st) {
        Some(c) => Type::Class {
            sym: c,
            args: vec![],
        },
        None => erase_ty(ty, st),
    }
}

fn erase_ty(ty: &Type, st: &SymbolTable) -> Type {
    // `Array[T]` reached through a classfile signature, or built by
    // substituting `Array` for a `C[_]` parameter, is `Class { array_sym }`.
    // Erasing that as a plain class emits the pseudo-name `[java/lang/Object`,
    // which no JVM will load.
    if let Some(a) = st.array_class_form(ty) {
        return erase_ty(&a, st);
    }
    match ty {
        Type::Class { sym, .. } if st.is_value_class(*sym) => {
            if let Some(u) = st.value_class_underlying(*sym) {
                erase_ty(&u, st)
            } else {
                Type::Any
            }
        }
        // nsc: a type parameter erases to the erasure of its upper bound.
        Type::TypeParam(_) => {
            let hi = st.widen_type_param(ty);
            if matches!(hi, Type::TypeParam(_)) {
                Type::Any
            } else {
                let e = erase_ty(&hi, st);
                if is_primitive(&e) {
                    Type::Any
                } else {
                    e
                }
            }
        }
        // An *alias* member erases like its right-hand side (`type Scope =
        // Map[K, V]` is `Map`). An **abstract** member erases like a type
        // parameter, to the erasure of its upper bound (SLS 3.7): slick's
        // `type RowsPerStatement >: One.type <: RowsPerStatement` is
        // `Lslick/jdbc/RowsPerStatement;`, and erasing it to `Object` instead
        // made the inherited `insertAll(Iterable, Object)` a different method
        // from the profile's `insertAll(Iterable, RowsPerStatement)` --
        // `NoSuchMethodError` on the trait's `$super$` accessor.
        //
        // Only a bound that names **one** class is taken. A compound bound
        // (`type TermName >: Null <: TermNameApi with Name`, from
        // scala-reflect) needs nsc's `intersectionDominator`, and guessing at
        // it is worse than `Object`: picking the first parent gave
        // `TermNameApi`, which is not a `NameApi`, so passing a `TermName`
        // where `Select.apply(TreeApi, NameApi)` wants a `Name` stopped
        // getting the cast that the `Object` erasure earns it and the macro
        // bridges failed to verify.
        Type::TypeMember(id) => match st.dealias(ty) {
            d if d == *ty => match st.get(*id).bound_hi.clone() {
                // `type A <: A` (or a bound naming the member itself) has no
                // more information than `Object`.
                Some(hi)
                    if hi != *ty
                        && !matches!(&hi, Type::TypeMember(h) if h == id)
                        && !matches!(st.dealias(&hi), Type::Refined { .. }) =>
                {
                    let e = erase_ty(&hi, st);
                    if is_primitive(&e) {
                        Type::Any
                    } else {
                        e
                    }
                }
                _ => Type::Any,
            },
            d => erase_ty(&d, st),
        },
        Type::Applied { ctor, .. } => erase_ty(ctor, st),
        // An existential's skolem erases like a type parameter: to the
        // erasure of its upper bound (SLS 3.7). slick declares
        // `lazy val shaped: ShapedValue[? <: E, E#TableElementType]`, so
        // `shaped.value` has type `? <: E`; erasing that to `Object` rather
        // than to `AbstractTable` left `def baseTableRow: E = shaped.value`
        // without the `checkcast` its own descriptor promises, and the
        // verifier threw the method out ("Bad return type") the first time
        // any program touched a `TableQuery`.
        Type::BoundedWildcard { hi: Some(hi), .. } => {
            let e = erase_ty(hi, st);
            if is_primitive(&e) {
                Type::Any
            } else {
                e
            }
        }
        Type::Wildcard | Type::BoundedWildcard { .. } => Type::Any,
        Type::Constant(lit) => Type::lit_underlying(lit),
        Type::ThisType(s) => Type::Class {
            sym: *s,
            args: vec![],
        },
        Type::SingleType { prefix, sym } => {
            let t = st.singleton_underlying(*sym);
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
            let e = erase_elem_ty(&args[0], st);
            if array_elem_is_abstract(&args[0])
                && matches!(e, Type::Any | Type::AnyRef | Type::AnyVal)
            {
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
            let e = erase_elem_ty(t, st);
            if array_elem_is_abstract(t) && matches!(e, Type::Any | Type::AnyRef | Type::AnyVal) {
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
        Type::Repeated(t) => Type::Repeated(Box::new(erase_elem_ty(t, st))),
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
        | Type::Byte
        | Type::Short
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
            | Type::Applied { .. }
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
                mark_value_class_patterns(&mut c.pat, st);
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
            // `x.asInstanceOf[T]` / `x.isInstanceOf[T]` are generic only in the
            // typechecker's eyes: `fun`'s own type is the *unsubstituted* type
            // parameter (only this outer node got `T` substituted in during
            // typing, in `type_expr`'s `TypeApply` case). The general rule
            // below — reuse `fun`'s erased type — would overwrite that
            // resolved `T` with the erased type parameter (its `Any` bound),
            // which is exactly the type backend codegen needs to know to cast
            // to. Every other generic method already erases to the same
            // `Object`-ish shape either way, so only these two need the
            // exception.
            let cast_ty = if fun.sym.is_none() {
                None
            } else {
                match st.get(fun.sym).intrinsic {
                    Intrinsic::AsInstanceOf | Intrinsic::IsInstanceOf => {
                        Some(erase_ty(&tree.ty, st))
                    }
                    _ => None,
                }
            };
            for a in args {
                // `classOf[Meters]` / `_: Meters` name the *boxed* class:
                // `Meters.class`, not `Integer.TYPE`.
                if let Some(c) = value_class_of(&a.ty, st) {
                    a.ty = Type::Class {
                        sym: c,
                        args: vec![],
                    };
                    continue;
                }
                erase_tree(a, st, None);
            }
            erase_tree(fun, st, None);
            // `x.asInstanceOf[Meters]` casts to the boxed class; the caller
            // unboxes from there if it wants the underlying value.
            let vc_cast = (!fun.sym.is_none()
                && matches!(st.get(fun.sym).intrinsic, Intrinsic::AsInstanceOf))
            .then(|| value_class_of(&tree.ty, st))
            .flatten();
            if let Some(c) = vc_cast {
                let orig = tree.ty.clone();
                tree.ty = Type::Class {
                    sym: c,
                    args: vec![],
                };
                adapt_box_unbox(tree, expected, &orig, st);
                return;
            }
            tree.ty = cast_ty.unwrap_or_else(|| fun.ty.clone());
        }
        TreeKind::Typed { expr, tpt } => {
            erase_tree(tpt, st, None);
            let pt = erase_ty(&tree.ty, st);
            erase_tree(expr, st, Some(&pt));
        }
        TreeKind::Select { qual, .. } => {
            // A member the value class itself declares runs on the underlying
            // value (`describe$extension(int)`); anything inherited from `Any`
            // -- `==`, `toString`, `hashCode` -- dispatches on a real instance,
            // so the receiver has to be boxed first.
            let recv_pt = match value_class_of(&qual.ty, st) {
                Some(c) if tree.sym.is_none() || st.get(tree.sym).owner != c => Some(Type::Any),
                _ => None,
            };
            erase_tree(qual, st, recv_pt.as_ref());
            if !tree.sym.is_none() {
                let owner = st.get(tree.sym).owner;
                if st.is_value_class(owner)
                    && st.get(owner).ctor_fields.first().copied() == Some(tree.sym)
                {
                    // The result is the underlying value, so what the caller
                    // may still have to box is the *field's* type, not the
                    // value class the selection was written on.
                    let field_ty = tree.ty.clone();
                    // A receiver that is already boxed -- an array element, a
                    // pattern binding -- keeps the selection: it is a plain
                    // field read off the instance.
                    if matches!(&qual.ty, Type::Class { sym, .. } if *sym == owner) {
                        tree.ty = erase_ty(&field_ty, st);
                        adapt_box_unbox(tree, expected, &field_ty, st);
                        return;
                    }
                    let q = std::mem::replace(qual, Box::new(Tree::dummy(TreeKind::Empty)));
                    let mut inner = *q;
                    inner.ty = erase_ty(&inner.ty, st);
                    *tree = inner;
                    adapt_box_unbox(tree, expected, &field_ty, st);
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
                    // `opt.get` on an `Option[Meters]` hands back the boxed
                    // instance, so the underlying comes out of the accessor.
                    if let Some(c) = value_class_of(&orig, st) {
                        if is_ref_erased(&ret_erased) {
                            let under = erase_ty(&orig, st);
                            tree.ty = ret_erased;
                            wrap_vc_unbox(tree, c, under);
                            adapt_box_unbox(tree, expected, &orig, st);
                            return;
                        }
                    }
                    if is_primitive(&orig)
                        && is_ref_erased(&ret_erased)
                        && !matches!(orig, Type::Unit)
                    {
                        tree.ty = ret_erased;
                        wrap_unbox(tree, orig.clone());
                        adapt_box_unbox(tree, expected, &orig, st);
                        return;
                    }
                    if matches!(orig, Type::String)
                        && is_ref_erased(&ret_erased)
                        && !matches!(ret_erased, Type::String)
                    {
                        // `List[String].head` erases to `()Object`; without the
                        // `$unbox` wrapper the checkcast to String is lost and
                        // `ws.head.length` fails verification. `erase_apply`
                        // already does this for the applied form.
                        tree.ty = ret_erased;
                        wrap_unbox(tree, orig.clone());
                        adapt_box_unbox(tree, expected, &orig, st);
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
                mark_value_class_patterns(&mut c.pat, st);
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
        TreeKind::Ident { name } if name == "$classOf" => {
            // The synthesized `ClassTag.apply(classOf[T])` argument carries the
            // element type. `classOf[Meters]` is `Meters.class`, not
            // `Integer.TYPE`: the array it tags holds boxed instances.
            if let Some(c) = value_class_of(&tree.ty, st) {
                tree.ty = Type::Class {
                    sym: c,
                    args: vec![],
                };
                return;
            }
        }
        TreeKind::Ident { .. } => {
            erase_ident(tree, st, expected);
        }
        _ => {}
    }

    let orig = tree.ty.clone();
    // A lambda parameter instantiated at a value class holds the boxed
    // instance (see `erase`); every reference to it has to agree.
    tree.ty = match boxed_value_class_ref(tree, st) {
        Some(t) => t,
        None => erase_ty(&orig, st),
    };
    if forwards_expected_to_result(&tree.kind) {
        // Already adapted through the branches: only record the type they now
        // have, and do not convert a second time.
        if let Some(a) = box_adaptation(&tree.ty, expected, &orig, st) {
            tree.ty = a.result_ty();
        }
        return;
    }
    adapt_box_unbox(tree, expected, &orig, st);
}

fn boxed_value_class_ref(tree: &Tree, st: &SymbolTable) -> Option<Type> {
    if !matches!(tree.kind, TreeKind::Ident { .. }) || tree.sym.is_none() {
        return None;
    }
    match &st.get(tree.sym).ty {
        t @ Type::Class { sym, .. } if st.is_value_class(*sym) => Some(t.clone()),
        _ => None,
    }
}

fn erase_ident(tree: &mut Tree, st: &SymbolTable, expected: Option<&Type>) {
    if tree.sym.is_none() {
        return;
    }
    let s = st.get(tree.sym);
    // A by-name parameter handed on to another by-name parameter keeps its
    // thunk. `def f[A](body: => A) = { def go(): A = body; go() }` lifts to
    // `go(body)`, and lambda-lift makes `go`'s new parameter *be* the same
    // by-name symbol -- so forcing the argument here passed the value and the
    // callee forced it a second time: `ClassCastException: java.lang.Integer
    // cannot be cast to scala.Function0` at the first call, from a compile
    // that reported nothing.
    if s.flags.contains(Flags::BYNAME)
        && matches!(&tree.ty, Type::ByName(_))
        && expected.is_some_and(is_thunk_slot)
    {
        tree.ty = erase_ty(&tree.ty, st);
        return;
    }
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

/// A parameter that takes the *thunk*: `=> T` before erasure, `Function0`
/// after.
fn is_thunk_slot(ty: &Type) -> bool {
    match ty {
        Type::ByName(_) => true,
        Type::Function { params, .. } => params.is_empty(),
        _ => false,
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
            let vc_ty = tree.ty.clone();
            erase_tree(&mut arg, st, Some(&erase_ty(&under, st)));
            *tree = arg;
            tree.ty = erase_ty(&under, st);
            // `new Meters(5)` in a reference position is a real `Meters`, not
            // an `Integer`: the box has to know which value class it is.
            adapt_box_unbox(tree, expected, &vc_ty, st);
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
        // The pre-erasure type still records which parameters were
        // *instantiated* at a value class (`Array[Meters].update(i, x)`,
        // `Array(m1, m2)`); after erasure that is indistinguishable from a
        // parameter declared as the value class itself, which takes the
        // underlying value.
        let fun_pre_ty = fun.ty.clone();
        erase_tree(fun, st, None);
        fun_ty = fun.ty.clone();
        param_tys = method_param_types(st, fun, &fun_pre_ty);
        let pre_params = flat_params(&fun_pre_ty);
        // This application calls whatever the callee *tree* denotes. Once that
        // tree's own type is a function type the call is `FunctionN.apply`,
        // and the symbol the tree carries stands for the callee only when it
        // is a value of that same function type. It is not the callee for the
        // inner `add(1)` of `add(1)(2)` (the symbol is `add`, whose result is
        // the *inner* application's), nor for the parameterless `f.tupled`
        // (whose result is the function now being applied) — reading the
        // result off either wraps the wrong unbox around this call.
        let sym_denotes_callee = match (&fun_pre_ty, &fun.kind) {
            (Type::Function { .. }, TreeKind::Apply { .. }) => false,
            (Type::Function { .. }, _) => {
                matches!(st.get(fun.sym).ty, Type::Function { .. })
            }
            _ => true,
        };
        if !fun.sym.is_none() && sym_denotes_callee {
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
            let vc_elem = vc_arg_expected(st, &pre_params, &param_tys, i);
            let p = vc_elem.or_else(|| param_tys.get(i).cloned());
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
            match value_class_of(&orig, st) {
                // `List[Meters].head` hands back a boxed `Meters`, so the
                // underlying comes out of the accessor, not `Integer.intValue`.
                Some(c) => wrap_vc_unbox(tree, c, orig_erased),
                None => wrap_unbox(tree, orig_erased),
            }
        }
    } else if matches!(orig_erased, Type::String)
        && is_ref_erased(&ret_erased)
        && !matches!(ret_erased, Type::String)
    {
        tree.ty = ret_erased;
        wrap_unbox(tree, orig_erased);
    }
    adapt_box_unbox(tree, expected, &orig, st);
}

/// Every declared parameter type, repeated clauses flattened.
fn flat_params(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        Type::Function { params, .. } => params.clone(),
        _ => Vec::new(),
    }
}

/// The parameter at index `i`, with a trailing repeated parameter spread over
/// the remaining arguments.
fn param_at(tys: &[Type], i: usize) -> Option<&Type> {
    match tys.get(i) {
        Some(Type::Repeated(e)) => Some(e),
        Some(t) => Some(t),
        None => match tys.last() {
            Some(Type::Repeated(e)) => Some(e),
            _ => None,
        },
    }
}

/// The expected type for an argument that was *instantiated* at a value class.
/// `Array(m1, m2)` and `arr(0) = m` store boxed instances, while `def show(m:
/// Meters)` takes the underlying value -- the declared erasure tells them
/// apart: a generic slot erases to a reference, a value-class slot does not.
fn vc_arg_expected(st: &SymbolTable, pre: &[Type], declared: &[Type], i: usize) -> Option<Type> {
    let c = value_class_of(param_at(pre, i)?, st)?;
    if let Some(d) = param_at(declared, i) {
        if !is_ref_erased(d) {
            return None;
        }
    }
    Some(Type::Class {
        sym: c,
        args: vec![],
    })
}

fn method_param_types(st: &SymbolTable, fun: &Tree, fun_pre_ty: &Type) -> Vec<Type> {
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
            // `Array[Meters].update(i, x)` stores into a `[LMeters;`, so the
            // value parameter is the *boxed* element type; that is only
            // visible before the substituted signature is erased.
            match fun_pre_ty {
                Type::Method { paramss, .. } => {
                    return paramss
                        .iter()
                        .flatten()
                        .map(|p| erase_elem_ty(p, st))
                        .collect();
                }
                Type::Function { params, .. } => {
                    return params.iter().map(|p| erase_elem_ty(p, st)).collect();
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

/// `case x: Meters =>` tests for a boxed `Meters`, not for the `Integer` the
/// value class erases to. Erasure would lose that, so the class is stamped on
/// the ascription node before the pattern is erased and the backend reads it
/// back from there.
fn mark_value_class_patterns(pat: &mut Tree, st: &SymbolTable) {
    if let TreeKind::Typed { .. } = &pat.kind {
        if let Some(c) = value_class_of(&pat.ty, st) {
            pat.sym = c;
        }
    }
    match &mut pat.kind {
        TreeKind::Typed { expr, .. } | TreeKind::Bind { body: expr, .. } => {
            mark_value_class_patterns(expr, st)
        }
        TreeKind::Alternative { trees } => {
            for t in trees {
                mark_value_class_patterns(t, st);
            }
        }
        TreeKind::UnApply { args, .. } | TreeKind::Apply { args, .. } => {
            for a in args {
                mark_value_class_patterns(a, st);
            }
        }
        _ => {}
    }
}

/// Record every value class this run compiles from source. Only those get the
/// boxed representation: the prelude models the library's own value classes
/// (`StringOps`, `ArrayOps`) as identity conversions over their underlying
/// value, and boxing one would hand `println` a `StringOps` where nsc has a
/// `String`. Called for every unit before any of them is erased, so a value
/// class is still boxed in the files that only *use* it.
pub fn note_source_value_classes(tree: &Tree, st: &mut SymbolTable) {
    let mut found = Vec::new();
    collect_source_value_classes(tree, st, &mut found);
    st.source_value_classes.extend(found);
    let mut all = Vec::new();
    collect_source_classes(tree, &mut all);
    st.source_classes.extend(all);
}

/// Every class and object this unit defines.
fn collect_source_classes(tree: &Tree, out: &mut Vec<SymbolId>) {
    if matches!(
        tree.kind,
        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
    ) && !tree.sym.is_none()
    {
        out.push(tree.sym);
    }
    for_each_child(tree, &mut |c| collect_source_classes(c, out));
}

fn collect_source_value_classes(tree: &Tree, st: &SymbolTable, out: &mut Vec<SymbolId>) {
    if matches!(tree.kind, TreeKind::ClassDef { .. })
        && !tree.sym.is_none()
        && st.is_value_class(tree.sym)
    {
        out.push(tree.sym);
    }
    for_each_child(tree, &mut |c| collect_source_value_classes(c, st, out));
}

/// The user-defined value class a *pre-erasure* type denotes, if any. The nine
/// primitive value classes are excluded: they have their own boxes.
pub(crate) fn value_class_of(ty: &Type, st: &SymbolTable) -> Option<SymbolId> {
    let sym = match ty {
        Type::Class { sym, .. } => *sym,
        Type::Applied { ctor, .. } => return value_class_of(ctor, st),
        Type::Annotated { tpe, .. } => return value_class_of(tpe, st),
        _ => return None,
    };
    (st.source_value_classes.contains(&sym) && st.is_value_class(sym)).then_some(sym)
}

/// The box/unbox conversion erasure owes a value of erased type `got` that has
/// to reach a position of erased type `expected`.
enum BoxAdapt {
    /// `BoxesRunTime.boxToInteger(x)` and friends.
    Box,
    /// `BoxesRunTime.unboxToInt(x)` and friends, to the named primitive.
    Unbox(Type),
    /// `new Meters(n)` -- the JVM-level box of a user value class.
    VcBox(SymbolId),
    /// `((Meters) x).n()` -- its unbox, to the underlying type.
    VcUnbox(SymbolId, Type),
}

impl BoxAdapt {
    /// The erased type the converted tree ends up with.
    fn result_ty(&self) -> Type {
        match self {
            BoxAdapt::Box => Type::Any,
            BoxAdapt::Unbox(to) => to.clone(),
            BoxAdapt::VcBox(c) => Type::Class {
                sym: *c,
                args: vec![],
            },
            BoxAdapt::VcUnbox(_, to) => to.clone(),
        }
    }
}

fn box_adaptation(
    got: &Type,
    expected: Option<&Type>,
    orig: &Type,
    st: &SymbolTable,
) -> Option<BoxAdapt> {
    let exp = expected?;
    // `class Meters(val n: Int) extends AnyVal` erases to `int`, but a value of
    // it that reaches a reference position -- `Any`, a universal trait it
    // implements, a type argument -- is a real `Meters` instance, not an
    // `Integer`. nsc's post-erasure `box`/`unbox` for value classes is
    // `new Meters(n)` / `((Meters) x).n()`.
    if let Some(c) = value_class_of(orig, st) {
        let under = st
            .value_class_underlying(c)
            .map(|u| erase_ty(&u, st))
            .unwrap_or(Type::Any);
        if !matches!(exp, Type::Unit) {
            // The underlying value goes where the underlying is asked for
            // (`describe$extension(int)`); every other reference position -- a
            // universal trait, `Any`, a type argument -- takes the instance.
            if *got == under && *exp != under && is_ref_erased(exp) {
                return Some(BoxAdapt::VcBox(c));
            }
            if *exp == under && *got != under && is_ref_erased(got) {
                return Some(BoxAdapt::VcUnbox(c, exp.clone()));
            }
        }
        return None;
    }
    if is_primitive(got) && is_ref_erased(exp) && !matches!(exp, Type::Unit) {
        return Some(BoxAdapt::Box);
    }
    if is_ref_erased(got) && is_primitive(exp) && !matches!(exp, Type::Unit) {
        return Some(BoxAdapt::Unbox(exp.clone()));
    }
    None
}

fn adapt_box_unbox(tree: &mut Tree, expected: Option<&Type>, orig: &Type, st: &SymbolTable) {
    match box_adaptation(&tree.ty, expected, orig, st) {
        Some(BoxAdapt::Box) => wrap_box(tree),
        Some(BoxAdapt::Unbox(to)) => wrap_unbox(tree, to),
        Some(BoxAdapt::VcBox(c)) => wrap_vc_box(tree, c),
        Some(BoxAdapt::VcUnbox(c, to)) => wrap_vc_unbox(tree, c, to),
        None => {}
    }
}

/// Whether erasure handed `expected` straight to the subexpressions that
/// produce this node's value, rather than to a node that yields one itself.
///
/// A `Block`'s value *is* its last expression's, an `If`'s is whichever branch
/// ran, a `Match`'s is the selected case body's, a `Try`'s is the body's or a
/// handler's. Each of those was erased against `expected` and so has already
/// been boxed or unboxed; the node is therefore adapted too, and running
/// `adapt_box_unbox` on it a second time boxes the box.
///
/// That is what made
/// `new It[Int] { def next(): Int = { val z = 1; z } }` emit
/// `boxToInteger(boxToInteger(z))` in the erased `next()Ljava/lang/Object;`
/// and fail verification, while the expression body `def next(): Int = z`
/// -- no block to descend into -- came out right.
fn forwards_expected_to_result(kind: &TreeKind) -> bool {
    matches!(
        kind,
        TreeKind::Block { .. }
            | TreeKind::If { .. }
            | TreeKind::Match { .. }
            | TreeKind::Try { .. }
    )
}

fn wrap_marker(tree: &mut Tree, name: &str, sym: SymbolId, param: Type, result: Type) {
    let span = tree.span;
    let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    let fun = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Ident { name: name.into() },
        ty: Type::Method {
            paramss: vec![vec![param]],
            ret: Box::new(result.clone()),
        },
        sym,
        postfix: false,
    };
    *tree = Tree {
        id: inner.id,
        span,
        kind: TreeKind::Apply {
            fun: Box::new(fun),
            args: vec![inner],
        },
        ty: result,
        sym,
        postfix: false,
    };
}

/// `new C(x)` -- the JVM-level box of a user value class.
fn wrap_vc_box(tree: &mut Tree, cls: SymbolId) {
    let under = tree.ty.clone();
    wrap_marker(
        tree,
        "$vcbox",
        cls,
        under,
        Type::Class {
            sym: cls,
            args: vec![],
        },
    );
}

/// `((C) x).u()` -- the JVM-level unbox of a user value class.
fn wrap_vc_unbox(tree: &mut Tree, cls: SymbolId, to: Type) {
    wrap_marker(tree, "$vcunbox", cls, Type::Any, to);
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
