//! The first executable specialization slice.
//!
//! This pass intentionally handles only one method-owned type parameter and
//! the Int/Long entries of `@specialized`.  It runs after the source pickle,
//! so the generic declaration (including its annotation) remains the source
//! ABI while the generated entries are JVM-only members.  Method variants are
//! limited to module methods, final methods, and private methods; an
//! overridable class method needs dispatch and bridge work that belongs to a
//! later phase.

use scala_rs_parser::{Flags, SpecializedType, SymbolId, Tree, TreeKind, Type};

use crate::symbol::{MethodVariant, SymKind, SymbolTable};

/// Add eligible primitive method variants to definitions in `tree`.
///
/// This is separate from [`rewrite_specialized_calls`] because a source unit
/// may call a method declared in a later source unit.  The driver runs this
/// collector for every unit before it rewrites any call site.
pub fn specialize_method_defs(tree: &mut Tree, st: &mut SymbolTable) {
    collect_defs(tree, st);
}

/// Rewrite concrete primitive calls to the JVM entry selected by the method's
/// type argument.  Calls whose type argument is unsupported, abstract, or
/// reference-typed keep their generic entry.
pub fn rewrite_specialized_calls(tree: &mut Tree, st: &SymbolTable) {
    rewrite_tree(tree, st);
}

fn collect_defs(tree: &mut Tree, st: &mut SymbolTable) {
    match &mut tree.kind {
        TreeKind::PackageDef { stats, .. } => {
            for stat in stats {
                collect_defs(stat, st);
            }
        }
        TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
            collect_template_body(&mut impl_.body, st);
            for parent in &mut impl_.parents {
                collect_defs(parent, st);
            }
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            for tp in tparams {
                collect_defs(tp, st);
            }
            for clause in vparamss {
                for param in clause {
                    collect_defs(param, st);
                }
            }
            collect_defs(tpt, st);
            collect_defs(rhs, st);
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            collect_defs(tpt, st);
            collect_defs(rhs, st);
        }
        TreeKind::Block { stats, expr } => {
            for stat in stats {
                collect_defs(stat, st);
            }
            collect_defs(expr, st);
        }
        TreeKind::If { cond, thenp, elsep } => {
            collect_defs(cond, st);
            collect_defs(thenp, st);
            collect_defs(elsep, st);
        }
        TreeKind::Match { selector, cases } => {
            collect_defs(selector, st);
            for case_def in cases {
                collect_defs(&mut case_def.pat, st);
                collect_defs(&mut case_def.guard, st);
                collect_defs(&mut case_def.body, st);
            }
        }
        TreeKind::Function { vparams, body } => {
            for param in vparams {
                collect_defs(param, st);
            }
            collect_defs(body, st);
        }
        TreeKind::Assign { lhs, rhs } => {
            collect_defs(lhs, st);
            collect_defs(rhs, st);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            collect_defs(cond, st);
            collect_defs(body, st);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            collect_defs(expr, st);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            collect_defs(block, st);
            for case_def in catches {
                collect_defs(&mut case_def.pat, st);
                collect_defs(&mut case_def.guard, st);
                collect_defs(&mut case_def.body, st);
            }
            collect_defs(finalizer, st);
        }
        TreeKind::Typed { expr, tpt } => {
            collect_defs(expr, st);
            collect_defs(tpt, st);
        }
        TreeKind::TypeApply { fun, args } | TreeKind::Apply { fun, args } => {
            collect_defs(fun, st);
            for arg in args {
                collect_defs(arg, st);
            }
        }
        TreeKind::Import { expr, .. }
        | TreeKind::Select { qual: expr, .. }
        | TreeKind::SelectFromTypeTree { qual: expr, .. }
        | TreeKind::Bind { body: expr, .. }
        | TreeKind::Star { elem: expr }
        | TreeKind::SingletonTypeTree { ref_: expr }
        | TreeKind::AnnotatedTypeTree { tpt: expr, .. } => collect_defs(expr, st),
        TreeKind::UnApply { fun, args } => {
            collect_defs(fun, st);
            for arg in args {
                collect_defs(arg, st);
            }
        }
        TreeKind::Alternative { trees } => {
            for child in trees {
                collect_defs(child, st);
            }
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            collect_defs(tpt, st);
            for arg in args {
                collect_defs(arg, st);
            }
        }
        TreeKind::CompoundTypeTree {
            parents,
            refinements,
        } => {
            for parent in parents {
                collect_defs(parent, st);
            }
            for refinement in refinements {
                collect_defs(refinement, st);
            }
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            collect_defs(tpt, st);
            for clause in clauses {
                collect_defs(clause, st);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for arg in args {
                collect_defs(arg, st);
            }
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            for param in params {
                collect_defs(param, st);
            }
            collect_defs(rhs, st);
        }
        TreeKind::Empty
        | TreeKind::Super { .. }
        | TreeKind::This { .. }
        | TreeKind::Ident { .. }
        | TreeKind::Literal { .. }
        | TreeKind::TypeDef { .. }
        | TreeKind::MacroRhs { .. }
        | TreeKind::Wildcard
        | TreeKind::Unimplemented { .. } => {}
    }
}

fn collect_template_body(body: &mut Vec<Tree>, st: &mut SymbolTable) {
    let original_len = body.len();
    let mut variants = Vec::new();
    for index in 0..original_len {
        let maybe_variants = if matches!(body[index].kind, TreeKind::DefDef { .. }) {
            build_variants(&body[index], st)
        } else {
            Vec::new()
        };
        variants.extend(maybe_variants);
        collect_defs(&mut body[index], st);
    }
    body.extend(variants);
}

fn build_variants(def: &Tree, st: &mut SymbolTable) -> Vec<Tree> {
    let TreeKind::DefDef {
        name, tparams, rhs, ..
    } = &def.kind
    else {
        return Vec::new();
    };
    if def.sym.is_none() || rhs.is_empty() {
        return Vec::new();
    }
    let original = def.sym;
    if st.method_variants.contains_key(&original) {
        return Vec::new();
    }
    let method = st.get(original);
    if method.kind != SymKind::Method
        || method.unspecialized
        || method.tparams.len() != 1
        || tparams.len() != 1
        || !eligible_owner(st, original)
    {
        return Vec::new();
    }
    let type_param = method.tparams[0];
    let Some(selected) = st.get(type_param).specialized else {
        return Vec::new();
    };
    let selected: Vec<SpecializedType> = selected
        .iter()
        .filter(|ty| matches!(ty, SpecializedType::Int | SpecializedType::Long))
        .collect();
    if selected.is_empty() {
        return Vec::new();
    }
    let original_ty = method.ty.clone();
    let original_params = method.params.clone();
    let original_paramss = method.paramss.clone();
    let original_flags = method.flags;
    let owner = method.owner;
    let mut out = Vec::new();
    let mut records = Vec::new();
    for selected_ty in selected {
        let primitive = primitive_type(selected_ty);
        let variant_name = format!("{name}$m{}c$sp", selected_ty.tag());
        let variant_ty = st.subst_tparams(original, &[primitive.clone()], &original_ty);
        let variant = st.alloc(&variant_name, owner, SymKind::Method, original_flags, "");
        {
            let symbol = st.get_mut(variant);
            symbol.ty = variant_ty.clone();
            symbol.params = original_params.clone();
            symbol.paramss = original_paramss.clone();
            symbol.tparams.clear();
        }
        let mut clone = def.clone();
        clone.sym = variant;
        clone.ty = variant_ty.clone();
        substitute_tree_types(&mut clone, st, original, &primitive);
        if let TreeKind::DefDef {
            mods,
            name: clone_name,
            tparams: clone_tparams,
            ..
        } = &mut clone.kind
        {
            *clone_name = variant_name.clone();
            clone_tparams.clear();
            mods.annotations.clear();
        }
        records.push(MethodVariant {
            original,
            symbol: variant,
            type_param,
            ty: variant_ty,
            jvm_name: variant_name,
        });
        out.push(clone);
    }
    st.method_variants.insert(original, records);
    out
}

fn eligible_owner(st: &SymbolTable, method: SymbolId) -> bool {
    let owner = st.get(method).owner;
    let owner_symbol = st.get(owner);
    matches!(owner_symbol.kind, SymKind::ModuleClass)
        || owner_symbol.flags.contains(Flags::FINAL)
        || st.get(method).flags.contains(Flags::PRIVATE)
        || st.get(method).flags.contains(Flags::FINAL)
}

fn primitive_type(ty: SpecializedType) -> Type {
    match ty {
        SpecializedType::Int => Type::Int,
        SpecializedType::Long => Type::Long,
        _ => unreachable!("the method slice selects Int and Long only"),
    }
}

fn variant_for(st: &SymbolTable, original: SymbolId, ty: &Type) -> Option<MethodVariant> {
    st.method_variants.get(&original).and_then(|variants| {
        variants
            .iter()
            .find(|variant| {
                let Type::Method { paramss, ret } = &variant.ty else {
                    return false;
                };
                paramss.iter().flatten().any(|param| param == ty) && **ret == *ty
            })
            .cloned()
    })
}

fn variant_for_method_type(
    st: &SymbolTable,
    original: SymbolId,
    method_ty: &Type,
) -> Option<MethodVariant> {
    st.method_variants
        .get(&original)
        .and_then(|variants| variants.iter().find(|variant| variant.ty == *method_ty))
        .cloned()
}

fn rewrite_tree(tree: &mut Tree, st: &SymbolTable) {
    match &mut tree.kind {
        TreeKind::TypeApply { fun, args } => {
            let original = fun.sym;
            if let Some(arg) = args.first().map(|arg| &arg.ty) {
                if matches!(arg, Type::Int | Type::Long) {
                    if let Some(variant) = variant_for(st, original, arg) {
                        let variant_id = variant.symbol;
                        let variant_name = variant.jvm_name.clone();
                        fun.sym = variant_id;
                        rename_fun(fun, &variant_name);
                        tree.sym = variant_id;
                        tree.ty = variant.ty.clone();
                    }
                }
            }
            rewrite_tree(fun, st);
            for arg in args {
                rewrite_tree(arg, st);
            }
        }
        TreeKind::PackageDef { stats, .. } => {
            for stat in stats {
                rewrite_tree(stat, st);
            }
        }
        TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
            for parent in &mut impl_.parents {
                rewrite_tree(parent, st);
            }
            for stat in &mut impl_.body {
                rewrite_tree(stat, st);
            }
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            rewrite_tree(tpt, st);
            rewrite_tree(rhs, st);
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            for tp in tparams {
                rewrite_tree(tp, st);
            }
            for clause in vparamss {
                for param in clause {
                    rewrite_tree(param, st);
                }
            }
            rewrite_tree(tpt, st);
            rewrite_tree(rhs, st);
        }
        TreeKind::Block { stats, expr } => {
            for stat in stats {
                rewrite_tree(stat, st);
            }
            rewrite_tree(expr, st);
        }
        TreeKind::If { cond, thenp, elsep } => {
            rewrite_tree(cond, st);
            rewrite_tree(thenp, st);
            rewrite_tree(elsep, st);
        }
        TreeKind::Match { selector, cases } => {
            rewrite_tree(selector, st);
            for case_def in cases {
                rewrite_tree(&mut case_def.pat, st);
                rewrite_tree(&mut case_def.guard, st);
                rewrite_tree(&mut case_def.body, st);
            }
        }
        TreeKind::Function { vparams, body } => {
            for param in vparams {
                rewrite_tree(param, st);
            }
            rewrite_tree(body, st);
        }
        TreeKind::Assign { lhs, rhs } => {
            rewrite_tree(lhs, st);
            rewrite_tree(rhs, st);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            rewrite_tree(cond, st);
            rewrite_tree(body, st);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            rewrite_tree(expr, st);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            rewrite_tree(block, st);
            for case_def in catches {
                rewrite_tree(&mut case_def.pat, st);
                rewrite_tree(&mut case_def.guard, st);
                rewrite_tree(&mut case_def.body, st);
            }
            rewrite_tree(finalizer, st);
        }
        TreeKind::Typed { expr, tpt } => {
            rewrite_tree(expr, st);
            rewrite_tree(tpt, st);
        }
        TreeKind::Apply { fun, args } => {
            rewrite_tree(fun, st);
            for arg in &mut *args {
                rewrite_tree(arg, st);
            }
            let original = fun.sym;
            if let Some(variant) = variant_for_method_type(st, original, &fun.ty) {
                let variant_id = variant.symbol;
                let variant_name = variant.jvm_name.clone();
                fun.sym = variant_id;
                rename_fun(fun, &variant_name);
                tree.sym = variant_id;
                tree.ty = method_result(&variant.ty);
                strip_boxed_arguments(args, &variant.ty);
            }
            strip_unbox_of_variant(tree, st);
        }
        TreeKind::UnApply { fun, args } => {
            rewrite_tree(fun, st);
            for arg in args {
                rewrite_tree(arg, st);
            }
        }
        TreeKind::Import { expr, .. }
        | TreeKind::Select { qual: expr, .. }
        | TreeKind::SelectFromTypeTree { qual: expr, .. }
        | TreeKind::Bind { body: expr, .. }
        | TreeKind::Star { elem: expr }
        | TreeKind::SingletonTypeTree { ref_: expr }
        | TreeKind::AnnotatedTypeTree { tpt: expr, .. } => rewrite_tree(expr, st),
        TreeKind::Alternative { trees } => {
            for child in trees {
                rewrite_tree(child, st);
            }
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            rewrite_tree(tpt, st);
            for arg in args {
                rewrite_tree(arg, st);
            }
        }
        TreeKind::CompoundTypeTree {
            parents,
            refinements,
        } => {
            for parent in parents {
                rewrite_tree(parent, st);
            }
            for refinement in refinements {
                rewrite_tree(refinement, st);
            }
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            rewrite_tree(tpt, st);
            for clause in clauses {
                rewrite_tree(clause, st);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for arg in args {
                rewrite_tree(arg, st);
            }
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            for param in params {
                rewrite_tree(param, st);
            }
            rewrite_tree(rhs, st);
        }
        TreeKind::Empty
        | TreeKind::Super { .. }
        | TreeKind::This { .. }
        | TreeKind::Ident { .. }
        | TreeKind::Literal { .. }
        | TreeKind::TypeDef { .. }
        | TreeKind::MacroRhs { .. }
        | TreeKind::Wildcard
        | TreeKind::Unimplemented { .. } => {}
    }
}

fn method_result(ty: &Type) -> Type {
    match ty {
        Type::Method { ret, .. } => (**ret).clone(),
        _ => Type::Any,
    }
}

fn strip_boxed_arguments(args: &mut [Tree], variant_ty: &Type) {
    let Type::Method { paramss, .. } = variant_ty else {
        return;
    };
    let params: Vec<&Type> = paramss.iter().flatten().collect();
    for (arg, param) in args.iter_mut().zip(params) {
        if !matches!(param, Type::Int | Type::Long) {
            continue;
        }
        let is_box = matches!(&arg.kind, TreeKind::Apply { fun, args } if fun.name() == Some("$box") && args.len() == 1);
        if !is_box {
            continue;
        }
        let old = std::mem::replace(arg, Tree::dummy(TreeKind::Empty));
        if let TreeKind::Apply { mut args, .. } = old.kind {
            if let Some(inner) = args.pop() {
                *arg = inner;
            }
        }
    }
}

fn strip_unbox_of_variant(tree: &mut Tree, st: &SymbolTable) {
    let TreeKind::Apply { fun, args } = &tree.kind else {
        return;
    };
    if fun.name() != Some("$unbox") || args.len() != 1 {
        return;
    }
    let Some(inner) = args.first() else {
        return;
    };
    let TreeKind::Apply { fun: inner_fun, .. } = &inner.kind else {
        return;
    };
    if inner_fun.sym.is_none() || !is_variant_symbol(st, inner_fun.sym) {
        return;
    }
    let outer_ty = tree.ty.clone();
    let old = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
    let TreeKind::Apply { mut args, .. } = old.kind else {
        unreachable!();
    };
    let mut inner = args.remove(0);
    inner.ty = outer_ty;
    *tree = inner;
}

fn is_variant_symbol(st: &SymbolTable, id: SymbolId) -> bool {
    st.method_variants
        .values()
        .any(|variants| variants.iter().any(|variant| variant.symbol == id))
}

fn rename_fun(fun: &mut Tree, name: &str) {
    match &mut fun.kind {
        TreeKind::Ident { name: old } | TreeKind::Select { name: old, .. } => {
            *old = name.to_string();
        }
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
            rename_fun(fun, name);
        }
        _ => {}
    }
}

fn substitute_tree_types(tree: &mut Tree, st: &SymbolTable, method: SymbolId, primitive: &Type) {
    tree.ty = st.subst_tparams(method, std::slice::from_ref(primitive), &tree.ty);
    match &mut tree.kind {
        TreeKind::PackageDef { pid, stats } => {
            substitute_tree_types(pid, st, method, primitive);
            for stat in stats {
                substitute_tree_types(stat, st, method, primitive);
            }
        }
        TreeKind::Import { expr, .. } => substitute_tree_types(expr, st, method, primitive),
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            for tp in tparams {
                substitute_tree_types(tp, st, method, primitive);
            }
            for clause in vparamss {
                for param in clause {
                    substitute_tree_types(param, st, method, primitive);
                }
            }
            for parent in &mut impl_.parents {
                substitute_tree_types(parent, st, method, primitive);
            }
            for stat in &mut impl_.body {
                substitute_tree_types(stat, st, method, primitive);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for parent in &mut impl_.parents {
                substitute_tree_types(parent, st, method, primitive);
            }
            for stat in &mut impl_.body {
                substitute_tree_types(stat, st, method, primitive);
            }
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            substitute_tree_types(tpt, st, method, primitive);
            substitute_tree_types(rhs, st, method, primitive);
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            for tp in tparams {
                substitute_tree_types(tp, st, method, primitive);
            }
            for clause in vparamss {
                for param in clause {
                    substitute_tree_types(param, st, method, primitive);
                }
            }
            substitute_tree_types(tpt, st, method, primitive);
            substitute_tree_types(rhs, st, method, primitive);
        }
        TreeKind::MacroRhs { impl_ref } => substitute_tree_types(impl_ref, st, method, primitive),
        TreeKind::TypeDef {
            tparams,
            rhs,
            lo,
            hi,
            views,
            ctx_bounds,
            ..
        } => {
            for tp in tparams {
                substitute_tree_types(tp, st, method, primitive);
            }
            substitute_tree_types(rhs, st, method, primitive);
            for bound in lo.iter_mut().chain(hi.iter_mut()) {
                substitute_tree_types(bound, st, method, primitive);
            }
            for view in views {
                substitute_tree_types(view, st, method, primitive);
            }
            for bound in ctx_bounds {
                substitute_tree_types(bound, st, method, primitive);
            }
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            for param in params {
                substitute_tree_types(param, st, method, primitive);
            }
            substitute_tree_types(rhs, st, method, primitive);
        }
        TreeKind::Block { stats, expr } => {
            for stat in stats {
                substitute_tree_types(stat, st, method, primitive);
            }
            substitute_tree_types(expr, st, method, primitive);
        }
        TreeKind::If { cond, thenp, elsep } => {
            substitute_tree_types(cond, st, method, primitive);
            substitute_tree_types(thenp, st, method, primitive);
            substitute_tree_types(elsep, st, method, primitive);
        }
        TreeKind::Match { selector, cases } => {
            substitute_tree_types(selector, st, method, primitive);
            for case_def in cases {
                substitute_tree_types(&mut case_def.pat, st, method, primitive);
                substitute_tree_types(&mut case_def.guard, st, method, primitive);
                substitute_tree_types(&mut case_def.body, st, method, primitive);
            }
        }
        TreeKind::Function { vparams, body } => {
            for param in vparams {
                substitute_tree_types(param, st, method, primitive);
            }
            substitute_tree_types(body, st, method, primitive);
        }
        TreeKind::Assign { lhs, rhs } => {
            substitute_tree_types(lhs, st, method, primitive);
            substitute_tree_types(rhs, st, method, primitive);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            substitute_tree_types(cond, st, method, primitive);
            substitute_tree_types(body, st, method, primitive);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            substitute_tree_types(expr, st, method, primitive);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            substitute_tree_types(block, st, method, primitive);
            for case_def in catches {
                substitute_tree_types(&mut case_def.pat, st, method, primitive);
                substitute_tree_types(&mut case_def.guard, st, method, primitive);
                substitute_tree_types(&mut case_def.body, st, method, primitive);
            }
            substitute_tree_types(finalizer, st, method, primitive);
        }
        TreeKind::Typed { expr, tpt } => {
            substitute_tree_types(expr, st, method, primitive);
            substitute_tree_types(tpt, st, method, primitive);
        }
        TreeKind::TypeApply { fun, args } | TreeKind::Apply { fun, args } => {
            substitute_tree_types(fun, st, method, primitive);
            for arg in args {
                substitute_tree_types(arg, st, method, primitive);
            }
        }
        TreeKind::Select { qual, .. }
        | TreeKind::SelectFromTypeTree { qual, .. }
        | TreeKind::Bind { body: qual, .. }
        | TreeKind::Star { elem: qual }
        | TreeKind::SingletonTypeTree { ref_: qual }
        | TreeKind::AnnotatedTypeTree { tpt: qual, .. } => {
            substitute_tree_types(qual, st, method, primitive);
        }
        TreeKind::UnApply { fun, args } => {
            substitute_tree_types(fun, st, method, primitive);
            for arg in args {
                substitute_tree_types(arg, st, method, primitive);
            }
        }
        TreeKind::Alternative { trees } => {
            for child in trees {
                substitute_tree_types(child, st, method, primitive);
            }
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            substitute_tree_types(tpt, st, method, primitive);
            for arg in args {
                substitute_tree_types(arg, st, method, primitive);
            }
        }
        TreeKind::CompoundTypeTree {
            parents,
            refinements,
        } => {
            for parent in parents {
                substitute_tree_types(parent, st, method, primitive);
            }
            for refinement in refinements {
                substitute_tree_types(refinement, st, method, primitive);
            }
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            substitute_tree_types(tpt, st, method, primitive);
            for clause in clauses {
                substitute_tree_types(clause, st, method, primitive);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for arg in args {
                substitute_tree_types(arg, st, method, primitive);
            }
        }
        TreeKind::Empty
        | TreeKind::Super { .. }
        | TreeKind::This { .. }
        | TreeKind::Ident { .. }
        | TreeKind::Literal { .. }
        | TreeKind::Wildcard
        | TreeKind::Unimplemented { .. } => {}
    }
}
