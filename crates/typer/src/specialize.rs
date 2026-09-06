//! The first executable specialization slice.
//!
//! This pass intentionally handles only one method-owned type parameter and
//! the Int/Long entries of `@specialized`.  It runs after the source pickle,
//! so the generic declaration (including its annotation) remains the source
//! ABI while the generated entries are JVM-only members.  Method variants are
//! limited to module methods, final methods, and private methods; an
//! overridable class method needs dispatch and bridge work that belongs to a
//! later phase.

use rustc_hash::FxHashMap;
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
    // nsc does not materialize entries when the type parameter is unused.
    // Without this guard Int and Long produce the same JVM signature, and an
    // inferred call with no surviving type argument would accidentally pick
    // the first entry; the duplicate-signature case is guarded below too.
    if st.subst_tparams(original, &[Type::Int], &original_ty) == original_ty {
        return Vec::new();
    }
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
            symbol.tparams.clear();
        }
        let mut clone = def.clone();
        clone.ty = variant_ty.clone();
        let symbol_map = clone_variant_symbols(&clone, st, original, variant, &primitive);
        clone.sym = variant;
        remap_tree_symbols(&mut clone, &symbol_map, original);
        substitute_tree_types(&mut clone, st, original, &primitive);
        let params: Vec<SymbolId> = original_params
            .iter()
            .map(|id| symbol_map.get(id).copied().unwrap_or(*id))
            .collect();
        let paramss: Vec<Vec<SymbolId>> = original_paramss
            .iter()
            .map(|clause| {
                clause
                    .iter()
                    .map(|id| symbol_map.get(id).copied().unwrap_or(*id))
                    .collect()
            })
            .collect();
        {
            let symbol = st.get_mut(variant);
            symbol.params = params;
            symbol.paramss = paramss;
        }
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
            selected: selected_ty,
            ty: variant_ty,
            jvm_name: variant_name,
        });
        out.push(clone);
    }
    st.method_variants.insert(original, records);
    out
}

/// Clone symbols defined inside a method body before substituting its type
/// parameter.  A typed tree clone alone is not enough: codegen uses parameter
/// and local symbol types when it allocates JVM slots, especially for lambda
/// bodies and nested definitions.  Sharing the generic symbols here would
/// leave an `Object` slot behind while the cloned tree asks for an `Int`.
fn clone_variant_symbols(
    tree: &Tree,
    st: &mut SymbolTable,
    original: SymbolId,
    variant: SymbolId,
    primitive: &Type,
) -> FxHashMap<SymbolId, SymbolId> {
    let mut map = FxHashMap::default();
    // The method symbol is also the target marker on `return` trees, but it
    // must remain the original symbol on call trees.  `rewrite_tree` uses that
    // original call symbol together with the call's actual type arguments to
    // choose a variant; mapping it here would turn `f[String]` inside an
    // `f[Int]` clone into another primitive call.
    map.insert(original, variant);
    collect_variant_symbols(tree, st, variant, original, primitive, &mut map);
    map
}

fn clone_one_symbol(
    old: SymbolId,
    owner: SymbolId,
    st: &mut SymbolTable,
    original: SymbolId,
    primitive: &Type,
    map: &mut FxHashMap<SymbolId, SymbolId>,
) -> SymbolId {
    if old.is_none() {
        return old;
    }
    if let Some(id) = map.get(&old) {
        return *id;
    }
    let source = st.get(old).clone();
    let id = st.alloc(
        source.name.clone(),
        owner,
        source.kind,
        source.flags,
        source.jvm_name.clone(),
    );
    // A local class is named after its enclosing source owner and receives a
    // per-owner index during namer (C$1, C$2, ...). A method variant has its
    // own class body, so reusing that JVM name makes the later clone overwrite
    // the generic local class on disk. Keep each clone addressable by assigning
    // the next local index before its body is emitted. Companion module names
    // carry the same index before their trailing $.
    let local_jvm = variant_local_jvm_name(st, &source);
    let mut copy = source;
    copy.id = id;
    copy.owner = owner;
    if let Some(jvm) = local_jvm {
        copy.jvm_name = jvm;
    }
    map.insert(old, id);
    copy.members.clear();
    copy.params.clear();
    copy.paramss.clear();
    copy.tparams.clear();
    copy.ty = st.subst_tparams(original, std::slice::from_ref(primitive), &copy.ty);
    remap_type_symbols(&mut copy.ty, map);
    copy.bound_lo = copy
        .bound_lo
        .map(|ty| st.subst_tparams(original, std::slice::from_ref(primitive), &ty));
    if let Some(ty) = &mut copy.bound_lo {
        remap_type_symbols(ty, map);
    }
    copy.bound_hi = copy
        .bound_hi
        .map(|ty| st.subst_tparams(original, std::slice::from_ref(primitive), &ty));
    if let Some(ty) = &mut copy.bound_hi {
        remap_type_symbols(ty, map);
    }
    *st.get_mut(id) = copy;
    id
}

fn variant_local_jvm_name(st: &SymbolTable, source: &crate::symbol::Symbol) -> Option<String> {
    if !matches!(
        source.kind,
        SymKind::Class | SymKind::ModuleClass | SymKind::Module
    ) || source.jvm_name.is_empty()
        || st.get(source.owner).kind != SymKind::Method
    {
        return None;
    }
    let trailing_dollar = source.jvm_name.ends_with('$');
    let stem = source
        .jvm_name
        .strip_suffix('$')
        .unwrap_or(&source.jvm_name);
    let (base, index) = stem.rsplit_once('$')?;
    let index = index.parse::<u32>().ok()?;
    let mut next = index.saturating_add(1);
    loop {
        let candidate = if trailing_dollar {
            [base, "$", &next.to_string(), "$"].concat()
        } else {
            [base, "$", &next.to_string()].concat()
        };
        if !st.symbols.iter().any(|sym| sym.jvm_name == candidate) {
            return Some(candidate);
        }
        next = next.saturating_add(1);
    }
}

fn remap_type_symbols(ty: &mut Type, map: &FxHashMap<SymbolId, SymbolId>) {
    match ty {
        Type::Array(elem) | Type::ByName(elem) | Type::Repeated(elem) => {
            remap_type_symbols(elem, map)
        }
        Type::Tuple(elems) => {
            for elem in elems {
                remap_type_symbols(elem, map);
            }
        }
        Type::Function { params, ret } => {
            for param in params {
                remap_type_symbols(param, map);
            }
            remap_type_symbols(ret, map);
        }
        Type::Named { args, .. } => {
            for arg in args {
                remap_type_symbols(arg, map);
            }
        }
        Type::Class { sym, args } => {
            if let Some(id) = map.get(sym) {
                *sym = *id;
            }
            for arg in args {
                remap_type_symbols(arg, map);
            }
        }
        Type::Method { paramss, ret } => {
            for clause in paramss {
                for param in clause {
                    remap_type_symbols(param, map);
                }
            }
            remap_type_symbols(ret, map);
        }
        Type::Overload(alts) => {
            for alt in alts {
                remap_type_symbols(alt, map);
            }
        }
        Type::ModuleRef(sym)
        | Type::TypeParam(sym)
        | Type::TypeMember(sym)
        | Type::ThisType(sym) => {
            if let Some(id) = map.get(sym) {
                *sym = *id;
            }
        }
        Type::Applied { ctor, args } => {
            remap_type_symbols(ctor, map);
            for arg in args {
                remap_type_symbols(arg, map);
            }
        }
        Type::SingleType { prefix, sym } => {
            remap_type_symbols(prefix, map);
            if let Some(id) = map.get(sym) {
                *sym = *id;
            }
        }
        Type::BoundedWildcard { lo, hi } => {
            if let Some(lo) = lo {
                remap_type_symbols(lo, map);
            }
            if let Some(hi) = hi {
                remap_type_symbols(hi, map);
            }
        }
        Type::Annotated { tpe, .. } => remap_type_symbols(tpe, map),
        Type::Refined { parents, decls } => {
            for parent in parents {
                remap_type_symbols(parent, map);
            }
            for decl in decls {
                match decl {
                    scala_rs_parser::RefineDecl::Type { rhs, lo, hi, .. } => {
                        if let Some(rhs) = rhs {
                            remap_type_symbols(rhs, map);
                        }
                        if let Some(lo) = lo {
                            remap_type_symbols(lo, map);
                        }
                        if let Some(hi) = hi {
                            remap_type_symbols(hi, map);
                        }
                    }
                    scala_rs_parser::RefineDecl::Def { paramss, ret, .. } => {
                        for clause in paramss {
                            for param in clause {
                                remap_type_symbols(param, map);
                            }
                        }
                        remap_type_symbols(ret, map);
                    }
                    scala_rs_parser::RefineDecl::Val { ty, .. } => {
                        remap_type_symbols(ty, map);
                    }
                }
            }
        }
        Type::NoType
        | Type::Error
        | Type::Unit
        | Type::Boolean
        | Type::Byte
        | Type::Short
        | Type::Int
        | Type::Long
        | Type::Float
        | Type::Double
        | Type::Char
        | Type::String
        | Type::Any
        | Type::AnyRef
        | Type::AnyVal
        | Type::Null
        | Type::Nothing
        | Type::Wildcard
        | Type::Constant(_) => {}
    }
}

fn collect_variant_symbols(
    tree: &Tree,
    st: &mut SymbolTable,
    current_owner: SymbolId,
    original: SymbolId,
    primitive: &Type,
    map: &mut FxHashMap<SymbolId, SymbolId>,
) {
    match &tree.kind {
        TreeKind::PackageDef { pid, stats } => {
            collect_variant_symbols(pid, st, current_owner, original, primitive, map);
            for stat in stats {
                collect_variant_symbols(stat, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            let owner = if tree.sym.is_none() {
                current_owner
            } else {
                clone_one_symbol(tree.sym, current_owner, st, original, primitive, map)
            };
            for tp in tparams {
                collect_variant_symbols(tp, st, owner, original, primitive, map);
            }
            for clause in vparamss {
                for param in clause {
                    collect_variant_symbols(param, st, owner, original, primitive, map);
                }
            }
            for parent in &impl_.parents {
                collect_variant_symbols(parent, st, owner, original, primitive, map);
            }
            for stat in &impl_.body {
                collect_variant_symbols(stat, st, owner, original, primitive, map);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            let owner = if tree.sym.is_none() {
                current_owner
            } else {
                clone_one_symbol(tree.sym, current_owner, st, original, primitive, map)
            };
            for parent in &impl_.parents {
                collect_variant_symbols(parent, st, owner, original, primitive, map);
            }
            for stat in &impl_.body {
                collect_variant_symbols(stat, st, owner, original, primitive, map);
            }
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            let owner = if tree.sym == original {
                current_owner
            } else if tree.sym.is_none() {
                current_owner
            } else {
                clone_one_symbol(tree.sym, current_owner, st, original, primitive, map)
            };
            let mut own_tparams = Vec::new();
            for tp in tparams {
                if !tp.sym.is_none() {
                    own_tparams.push(clone_one_symbol(
                        tp.sym, owner, st, original, primitive, map,
                    ));
                }
                collect_variant_symbols(tp, st, owner, original, primitive, map);
            }
            let mut own_paramss = Vec::new();
            for clause in vparamss {
                let mut params = Vec::new();
                for param in clause {
                    if !param.sym.is_none() {
                        params.push(clone_one_symbol(
                            param.sym, owner, st, original, primitive, map,
                        ));
                    }
                    collect_variant_symbols(param, st, owner, original, primitive, map);
                }
                own_paramss.push(params);
            }
            if tree.sym != original && !tree.sym.is_none() {
                let symbol = st.get_mut(owner);
                symbol.tparams = own_tparams;
                symbol.paramss = own_paramss.clone();
                symbol.params = own_paramss.into_iter().flatten().collect();
            }
            collect_variant_symbols(tpt, st, owner, original, primitive, map);
            collect_variant_symbols(rhs, st, owner, original, primitive, map);
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            if !tree.sym.is_none() {
                clone_one_symbol(tree.sym, current_owner, st, original, primitive, map);
            }
            collect_variant_symbols(tpt, st, current_owner, original, primitive, map);
            collect_variant_symbols(rhs, st, current_owner, original, primitive, map);
        }
        TreeKind::Function { vparams, body } => {
            for param in vparams {
                if !param.sym.is_none() {
                    clone_one_symbol(param.sym, current_owner, st, original, primitive, map);
                }
                collect_variant_symbols(param, st, current_owner, original, primitive, map);
            }
            collect_variant_symbols(body, st, current_owner, original, primitive, map);
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            for param in params {
                if !param.sym.is_none() {
                    clone_one_symbol(param.sym, current_owner, st, original, primitive, map);
                }
                collect_variant_symbols(param, st, current_owner, original, primitive, map);
            }
            collect_variant_symbols(rhs, st, current_owner, original, primitive, map);
        }
        TreeKind::Bind { body, .. } => {
            if !tree.sym.is_none() {
                clone_one_symbol(tree.sym, current_owner, st, original, primitive, map);
            }
            collect_variant_symbols(body, st, current_owner, original, primitive, map);
        }
        TreeKind::Block { stats, expr } => {
            for stat in stats {
                collect_variant_symbols(stat, st, current_owner, original, primitive, map);
            }
            collect_variant_symbols(expr, st, current_owner, original, primitive, map);
        }
        TreeKind::If { cond, thenp, elsep } => {
            collect_variant_symbols(cond, st, current_owner, original, primitive, map);
            collect_variant_symbols(thenp, st, current_owner, original, primitive, map);
            collect_variant_symbols(elsep, st, current_owner, original, primitive, map);
        }
        TreeKind::Match { selector, cases } => {
            collect_variant_symbols(selector, st, current_owner, original, primitive, map);
            for case_def in cases {
                collect_variant_symbols(&case_def.pat, st, current_owner, original, primitive, map);
                collect_variant_symbols(
                    &case_def.guard,
                    st,
                    current_owner,
                    original,
                    primitive,
                    map,
                );
                collect_variant_symbols(
                    &case_def.body,
                    st,
                    current_owner,
                    original,
                    primitive,
                    map,
                );
            }
        }
        TreeKind::Assign { lhs, rhs } => {
            collect_variant_symbols(lhs, st, current_owner, original, primitive, map);
            collect_variant_symbols(rhs, st, current_owner, original, primitive, map);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            collect_variant_symbols(cond, st, current_owner, original, primitive, map);
            collect_variant_symbols(body, st, current_owner, original, primitive, map);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            collect_variant_symbols(expr, st, current_owner, original, primitive, map);
        }
        TreeKind::New { tpt } => {
            if !tree.sym.is_none() {
                let owner = map
                    .get(&st.get(tree.sym).owner)
                    .copied()
                    .unwrap_or(current_owner);
                clone_one_symbol(tree.sym, owner, st, original, primitive, map);
            }
            collect_variant_symbols(tpt, st, current_owner, original, primitive, map);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            collect_variant_symbols(block, st, current_owner, original, primitive, map);
            for case_def in catches {
                collect_variant_symbols(&case_def.pat, st, current_owner, original, primitive, map);
                collect_variant_symbols(
                    &case_def.guard,
                    st,
                    current_owner,
                    original,
                    primitive,
                    map,
                );
                collect_variant_symbols(
                    &case_def.body,
                    st,
                    current_owner,
                    original,
                    primitive,
                    map,
                );
            }
            collect_variant_symbols(finalizer, st, current_owner, original, primitive, map);
        }
        TreeKind::Typed { expr, tpt } => {
            collect_variant_symbols(expr, st, current_owner, original, primitive, map);
            collect_variant_symbols(tpt, st, current_owner, original, primitive, map);
        }
        TreeKind::TypeApply { fun, args } => {
            collect_variant_symbols(fun, st, current_owner, original, primitive, map);
            for arg in args {
                collect_variant_symbols(arg, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::Apply { fun, args } => {
            // Constructor symbols live on the Apply around New, rather than
            // on the New tree itself. Clone them under the cloned local class
            // so gen_new derives the primitive constructor descriptor.
            if !tree.sym.is_none() && st.get(tree.sym).name == "<init>" {
                let owner = map
                    .get(&st.get(tree.sym).owner)
                    .copied()
                    .unwrap_or(current_owner);
                clone_one_symbol(tree.sym, owner, st, original, primitive, map);
            }
            collect_variant_symbols(fun, st, current_owner, original, primitive, map);
            for arg in args {
                collect_variant_symbols(arg, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::Import { expr, .. }
        | TreeKind::Select { qual: expr, .. }
        | TreeKind::SelectFromTypeTree { qual: expr, .. }
        | TreeKind::Star { elem: expr }
        | TreeKind::SingletonTypeTree { ref_: expr }
        | TreeKind::AnnotatedTypeTree { tpt: expr, .. } => {
            collect_variant_symbols(expr, st, current_owner, original, primitive, map)
        }
        TreeKind::UnApply { fun, args } => {
            collect_variant_symbols(fun, st, current_owner, original, primitive, map);
            for arg in args {
                collect_variant_symbols(arg, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::Alternative { trees } => {
            for child in trees {
                collect_variant_symbols(child, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            collect_variant_symbols(tpt, st, current_owner, original, primitive, map);
            for arg in args {
                collect_variant_symbols(arg, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::CompoundTypeTree {
            parents,
            refinements,
        } => {
            for parent in parents {
                collect_variant_symbols(parent, st, current_owner, original, primitive, map);
            }
            for refinement in refinements {
                collect_variant_symbols(refinement, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            collect_variant_symbols(tpt, st, current_owner, original, primitive, map);
            for clause in clauses {
                collect_variant_symbols(clause, st, current_owner, original, primitive, map);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for arg in args {
                collect_variant_symbols(arg, st, current_owner, original, primitive, map);
            }
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

fn remap_tree_symbols(tree: &mut Tree, map: &FxHashMap<SymbolId, SymbolId>, original: SymbolId) {
    // Apply/TypeApply/Select/Ident symbols identify the callee being called.
    // Keep those references on the generic method so rewrite_tree can inspect
    // their actual type arguments.  Other symbol-bearing trees include local
    // definitions and Return's non-local-return target and should be remapped.
    let is_call_reference = matches!(
        &tree.kind,
        TreeKind::TypeApply { .. }
            | TreeKind::Apply { .. }
            | TreeKind::Select { .. }
            | TreeKind::SelectFromTypeTree { .. }
            | TreeKind::Ident { .. }
    );
    if !is_call_reference || tree.sym != original {
        if let Some(id) = map.get(&tree.sym) {
            tree.sym = *id;
        }
    }
    match &mut tree.kind {
        TreeKind::PackageDef { pid, stats } => {
            remap_tree_symbols(pid, map, original);
            for stat in stats {
                remap_tree_symbols(stat, map, original);
            }
        }
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            for tp in tparams {
                remap_tree_symbols(tp, map, original);
            }
            for clause in vparamss {
                for param in clause {
                    remap_tree_symbols(param, map, original);
                }
            }
            for parent in &mut impl_.parents {
                remap_tree_symbols(parent, map, original);
            }
            for stat in &mut impl_.body {
                remap_tree_symbols(stat, map, original);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for parent in &mut impl_.parents {
                remap_tree_symbols(parent, map, original);
            }
            for stat in &mut impl_.body {
                remap_tree_symbols(stat, map, original);
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
                remap_tree_symbols(tp, map, original);
            }
            for clause in vparamss {
                for param in clause {
                    remap_tree_symbols(param, map, original);
                }
            }
            remap_tree_symbols(tpt, map, original);
            remap_tree_symbols(rhs, map, original);
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            remap_tree_symbols(tpt, map, original);
            remap_tree_symbols(rhs, map, original);
        }
        TreeKind::Block { stats, expr } => {
            for stat in stats {
                remap_tree_symbols(stat, map, original);
            }
            remap_tree_symbols(expr, map, original);
        }
        TreeKind::If { cond, thenp, elsep } => {
            remap_tree_symbols(cond, map, original);
            remap_tree_symbols(thenp, map, original);
            remap_tree_symbols(elsep, map, original);
        }
        TreeKind::Match { selector, cases } => {
            remap_tree_symbols(selector, map, original);
            for case_def in cases {
                remap_tree_symbols(&mut case_def.pat, map, original);
                remap_tree_symbols(&mut case_def.guard, map, original);
                remap_tree_symbols(&mut case_def.body, map, original);
            }
        }
        TreeKind::Function { vparams, body } => {
            for param in vparams {
                remap_tree_symbols(param, map, original);
            }
            remap_tree_symbols(body, map, original);
        }
        TreeKind::Assign { lhs, rhs } => {
            remap_tree_symbols(lhs, map, original);
            remap_tree_symbols(rhs, map, original);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            remap_tree_symbols(cond, map, original);
            remap_tree_symbols(body, map, original);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            remap_tree_symbols(expr, map, original);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            remap_tree_symbols(block, map, original);
            for case_def in catches {
                remap_tree_symbols(&mut case_def.pat, map, original);
                remap_tree_symbols(&mut case_def.guard, map, original);
                remap_tree_symbols(&mut case_def.body, map, original);
            }
            remap_tree_symbols(finalizer, map, original);
        }
        TreeKind::Typed { expr, tpt } => {
            remap_tree_symbols(expr, map, original);
            remap_tree_symbols(tpt, map, original);
        }
        TreeKind::TypeApply { fun, args } | TreeKind::Apply { fun, args } => {
            remap_tree_symbols(fun, map, original);
            for arg in args {
                remap_tree_symbols(arg, map, original);
            }
        }
        TreeKind::Import { expr, .. }
        | TreeKind::Select { qual: expr, .. }
        | TreeKind::SelectFromTypeTree { qual: expr, .. }
        | TreeKind::Bind { body: expr, .. }
        | TreeKind::Star { elem: expr }
        | TreeKind::SingletonTypeTree { ref_: expr }
        | TreeKind::AnnotatedTypeTree { tpt: expr, .. } => remap_tree_symbols(expr, map, original),
        TreeKind::UnApply { fun, args } => {
            remap_tree_symbols(fun, map, original);
            for arg in args {
                remap_tree_symbols(arg, map, original);
            }
        }
        TreeKind::Alternative { trees } => {
            for child in trees {
                remap_tree_symbols(child, map, original);
            }
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            remap_tree_symbols(tpt, map, original);
            for arg in args {
                remap_tree_symbols(arg, map, original);
            }
        }
        TreeKind::CompoundTypeTree {
            parents,
            refinements,
        } => {
            for parent in parents {
                remap_tree_symbols(parent, map, original);
            }
            for refinement in refinements {
                remap_tree_symbols(refinement, map, original);
            }
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            remap_tree_symbols(tpt, map, original);
            for clause in clauses {
                remap_tree_symbols(clause, map, original);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for arg in args {
                remap_tree_symbols(arg, map, original);
            }
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            for param in params {
                remap_tree_symbols(param, map, original);
            }
            remap_tree_symbols(rhs, map, original);
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
            .find(|variant| primitive_type(variant.selected) == *ty)
            .cloned()
    })
}

fn variant_for_method_type(
    st: &SymbolTable,
    original: SymbolId,
    method_ty: &Type,
) -> Option<MethodVariant> {
    let variants = st.method_variants.get(&original)?;
    let mut matches = variants.iter().filter(|variant| variant.ty == *method_ty);
    let first = matches.next()?.clone();
    // A type argument can be erased before this pass reaches a direct Apply.
    // If more than one primitive entry has the same method type, there is no
    // sound way to recover that argument from the call shape; keep the generic
    // call instead of selecting Int by iteration order.
    if matches.next().is_some() {
        None
    } else {
        Some(first)
    }
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
