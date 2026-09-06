//! Erasure and the JVM vocabulary the rest of the emitter speaks: sorts and
//! descriptors for Scala types, method descriptors from symbols and trees,
//! erasure-bridge adaptation, constructor descriptors, the `$outer` chain and
//! module addressing, the expanded names nsc gives mixin members, parent
//! splitting for a template, access flags, and the primitive-conversion and
//! descriptor-parsing helpers at the bottom of the file.

use crate::classfile::{
    encode_method_name, ACC_FINAL, ACC_NATIVE, ACC_PRIVATE, ACC_PUBLIC, ACC_STATIC, ACC_TRANSIENT,
    ACC_VOLATILE,
};
use crate::code::Assembler;
use crate::gen::*;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{SymKind, SymbolTable};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// descriptors
// ---------------------------------------------------------------------------

pub(crate) fn jvm_sort(ty: &Type) -> JvmSort {
    match ty {
        Type::Unit | Type::NoType | Type::Nothing => JvmSort::Void,
        Type::Boolean | Type::Int | Type::Char | Type::Byte | Type::Short => JvmSort::Int,
        Type::Long => JvmSort::Long,
        Type::Float => JvmSort::Float,
        Type::Double => JvmSort::Double,
        Type::Constant(lit) => jvm_sort(&Type::lit_underlying(lit)),
        _ => JvmSort::Ref,
    }
}

pub(crate) fn is_unit_like(ty: &Type) -> bool {
    matches!(ty, Type::Unit | Type::NoType)
}

/// Sort of a value held in a *slot* — a parameter or a local. Differs from
/// `jvm_sort` only for the two types that are `V` as a method result but a
/// real reference wherever a value is passed or stored: `Unit`
/// (`scala/runtime/BoxedUnit`) and `Nothing` (`scala/runtime/Nothing$`).
/// Pass-through code (forwarders, bridges, setters) uses this so it moves the
/// argument it was handed; a method *body* keeps the void sort and only
/// reserves the slot, see `Frame::alloc_param`.
pub(crate) fn jvm_slot_sort(ty: &Type) -> JvmSort {
    if erases_to_ref_slot(ty) {
        JvmSort::Ref
    } else {
        jvm_sort(ty)
    }
}

/// A type whose *result* erasure is `V` but whose *value* erasure is a
/// reference, so it occupies a parameter slot.
pub(crate) fn erases_to_ref_slot(ty: &Type) -> bool {
    matches!(
        ty.widen_constant(),
        Type::Unit | Type::NoType | Type::Nothing
    )
}

/// How many local slots a parameter of this type occupies.
pub(crate) fn param_slots(ty: &Type) -> u16 {
    jvm_slot_sort(ty).slots()
}

pub(crate) fn class_internal(st: &SymbolTable, id: SymbolId) -> String {
    st.jvm_internal(id)
}

/// `Unit` is `V` only as a method *result*. Everywhere a value actually lives
/// -- a parameter, a field, an array element, a type argument -- nsc erases it
/// to `scala/runtime/BoxedUnit`, whose sole instance is `BoxedUnit.UNIT`.
/// A descriptor is not even well-formed with a bare `V` in those positions:
/// `def f(x: Unit)` came out as `(V)Ljava/lang/String;` and the JVM rejected
/// the whole class with `ClassFormatError: illegal signature`.
pub(crate) const BOXED_UNIT: &str = "scala/runtime/BoxedUnit";
pub(crate) const BOXED_UNIT_DESC: &str = "Lscala/runtime/BoxedUnit;";

/// `Nothing`'s erasure. Unlike `Unit` it is this class in *every* position,
/// result included, and it is a subtype of nothing at all -- which is what
/// makes handing such a value on a `VerifyError` rather than a mere
/// infelicity.
pub(crate) const NOTHING_CLASS: &str = "scala/runtime/Nothing$";
pub(crate) const NOTHING_DESC: &str = "Lscala/runtime/Nothing$;";

/// Erasure of a type in a *value* position, as opposed to a method result.
/// `Nothing` has the same problem as `Unit` and nsc gives it its own class.
pub(crate) fn jvm_desc_val(st: &SymbolTable, ty: &Type) -> String {
    match ty.widen_constant() {
        Type::Unit | Type::NoType => BOXED_UNIT_DESC.into(),
        Type::Nothing => NOTHING_DESC.into(),
        _ => jvm_desc(st, ty),
    }
}

/// Array elements are a value position too, so `Array[Unit]` is
/// `[Lscala/runtime/BoxedUnit;`. The two bottom types are the exception nsc
/// makes: `Array[Nothing]` and `Array[Null]` erase to `Object[]`, not to
/// `Nothing$[]` / `Null$[]` (`erasure.scala`'s `arrayType` special case;
/// confirmed with `javap -s` on scalac 2.13.16 output for
/// `def arr: Array[Null] = new Array[Null](0)`, which is `()[Ljava/lang/Object;`).
pub(crate) fn jvm_desc_array_elem(st: &SymbolTable, ty: &Type) -> String {
    match ty.widen_constant() {
        Type::Nothing | Type::Null => "Ljava/lang/Object;".into(),
        _ => jvm_desc_val(st, ty),
    }
}

/// True when this type needs `BoxedUnit.UNIT` materialised to occupy the value
/// position it was erased into.
pub(crate) fn erases_to_boxed_unit(ty: &Type) -> bool {
    matches!(ty.widen_constant(), Type::Unit | Type::NoType)
}

pub(crate) fn jvm_desc(st: &SymbolTable, ty: &Type) -> String {
    match ty {
        Type::Unit | Type::NoType => "V".into(),
        // `Unit` is `V` only as a method *result* (see `BOXED_UNIT` above);
        // `Nothing` gets no such treatment from nsc even there -- `def die():
        // Nothing = throw ...` still compiles to `()Lscala/runtime/Nothing$;`
        // (confirmed with `javap -c` on real scalac output), never `()V`. A
        // caller invoking such a method needs that descriptor to know a real
        // reference lands on the stack, for `gen_expr`'s `athrow`-append
        // (see its doc comment) to consume.
        Type::Nothing => NOTHING_DESC.into(),
        // The other bottom type gets the same treatment, and this one really
        // is an ABI question rather than a verifier one: nsc erases `Null` to
        // `scala/runtime/Null$` in every position (`def n: Null` is
        // `()Lscala/runtime/Null$;`, `def take(x: Null)` is
        // `(Lscala/runtime/Null$;)I`, a `val` field is `Lscala/runtime/Null$;`),
        // so erasing it to `Object` made every separately compiled signature
        // disagree with scalac's -- including the generic ones, where
        // `List[Null]` came out `List<Object>` against nsc's `List<Null$>`.
        Type::Null => "Lscala/runtime/Null$;".into(),
        Type::Boolean => "Z".into(),
        Type::Byte => "B".into(),
        Type::Short => "S".into(),
        Type::Int => "I".into(),
        Type::Long => "J".into(),
        Type::Float => "F".into(),
        Type::Double => "D".into(),
        Type::Char => "C".into(),
        Type::String => "Ljava/lang/String;".into(),
        Type::Array(t) => format!("[{}", jvm_desc_array_elem(st, t)),
        Type::Class { sym, .. } => format!("L{};", class_internal(st, *sym)),
        Type::ModuleRef(sym) => format!("L{};", class_internal(st, *sym)),
        Type::Any | Type::AnyRef | Type::AnyVal | Type::Error => "Ljava/lang/Object;".into(),
        Type::Function { params, .. } => format!("Lscala/Function{};", params.len()),
        Type::Tuple(ts) => format!("Lscala/Tuple{};", ts.len()),
        Type::Method { ret, .. } => jvm_desc(st, ret),
        Type::ByName(_) => "Lscala/Function0;".into(),
        Type::Repeated(_) => "Lscala/collection/immutable/Seq;".into(),
        Type::TypeParam(_)
        | Type::TypeMember(_)
        | Type::Applied { .. }
        | Type::Wildcard
        | Type::BoundedWildcard { .. } => "Ljava/lang/Object;".into(),
        Type::ThisType(sym) => format!("L{};", class_internal(st, *sym)),
        Type::Constant(lit) => jvm_desc(st, &Type::lit_underlying(lit)),
        Type::SingleType { prefix, sym } => {
            let inner = st.get(*sym).ty.clone();
            if inner.is_no_type() {
                jvm_desc(st, prefix)
            } else {
                jvm_desc(st, &inner)
            }
        }
        Type::Annotated { tpe, .. } => jvm_desc(st, tpe),
        Type::Refined { .. } => "Ljava/lang/Object;".into(),
        Type::Named { name, args } if name == "Array" && args.len() == 1 => {
            format!("[{}", jvm_desc_array_elem(st, &args[0]))
        }
        Type::Named { name, .. } => {
            let n = name.replace('.', "/");
            format!("L{n};")
        }
        Type::Overload(_) => "Ljava/lang/Object;".into(),
    }
}

pub(crate) fn jvm_method_desc(st: &SymbolTable, params: &[Type], ret: &Type) -> String {
    let mut s = String::from("(");
    for p in params {
        s.push_str(&jvm_desc_val(st, p));
    }
    s.push(')');
    s.push_str(&jvm_desc(st, ret));
    s
}

pub(crate) fn method_ret_ty(def: &Tree) -> Type {
    match &def.ty {
        Type::Method { ret, .. } => (**ret).clone(),
        Type::Function { ret, .. } => (**ret).clone(),
        t if !t.is_no_type() => t.clone(),
        _ => Type::Unit,
    }
}

pub(crate) fn def_param_types(st: &SymbolTable, def: &Tree) -> Vec<Type> {
    match &def.kind {
        TreeKind::DefDef { vparamss, .. } => vparamss
            .iter()
            .flatten()
            .map(|p| {
                if !p.ty.is_no_type() && !p.ty.is_error() {
                    p.ty.clone()
                } else if !p.sym.is_none() {
                    st.get(p.sym).ty.clone()
                } else {
                    Type::Any
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn def_method_desc(st: &SymbolTable, def: &Tree) -> String {
    jvm_method_desc(st, &def_param_types(st, def), &method_ret_ty(def))
}

pub(crate) fn method_params_from_sym(st: &SymbolTable, id: SymbolId) -> Vec<Type> {
    let s = st.get(id);
    match &s.ty {
        Type::Method { paramss, .. } => {
            let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
            if params.iter().any(|p| p.is_no_type() || p.is_error()) {
                s.params.iter().map(|p| st.get(*p).ty.clone()).collect()
            } else {
                params
            }
        }
        Type::Function { params, .. } => params.clone(),
        _ => s.params.iter().map(|p| st.get(*p).ty.clone()).collect(),
    }
}

/// Whether a subclass method with `child` parameters overrides a parent one
/// with `parent` parameters, rather than merely overloading it. A bridge is
/// only owed for an override: `class Derived extends Base { def f(s: String) }`
/// next to `Base.f(x: Int)` adds an alternative, and a `f(I)` bridge
/// forwarding to `f(String)` is not verifiable code.
///
/// A parent parameter that mentions a type parameter or an abstract type
/// member is exactly the case a bridge exists for (`def f(x: A)` implemented
/// as `f(x: Int)`), so it matches anything.
///
/// Two parameters that erase to the same descriptor are the same JVM parameter
/// however differently they are written, so they cannot be what tells two
/// overloads apart either: `def bind[A, B](fa: F[A])(f: A => F[B])`
/// implemented at `F = Option` declares `f: A => Option[B]`, and both are
/// `Lscala/Function1;`. Comparing those structurally said "not an override",
/// no bridge was emitted, and calling `bind` through the interface threw
/// `AbstractMethodError`. (When *every* parameter matches this way the two
/// descriptors are equal and the caller skips the bridge anyway.)
/// `parent_abstract` is `SymbolTable::erased_abstract_params` for the parent
/// method: an abstract parameter whose *bound* is a class does not erase to
/// `Object`, so nothing in the erased types themselves says the subclass
/// narrowed it. slick's `MappedColumnTypeFactory.base(…, BaseColumnType[U])`
/// erases to `TypedType` and `MappedJdbcType`'s implementation to `JdbcType`;
/// without the mask that read as an unrelated overload, no bridge was emitted,
/// and the interface method stayed abstract (`AbstractMethodError`).
pub(crate) fn bridge_overrides(
    st: &SymbolTable,
    parent: &[Type],
    child: &[Type],
    parent_abstract: u32,
) -> bool {
    parent.len() == child.len()
        && parent.iter().zip(child).enumerate().all(|(i, (p, c))| {
            p == c
                || jvm_desc(st, p) == jvm_desc(st, c)
                || erases_to_object(st, p)
                || erases_to_object(st, c)
                || (i < 32 && parent_abstract & (1 << i) != 0)
        })
}

/// A parameter that carries no information after erasure. This is precisely
/// the shape a bridge exists for: `def show(a: A)` erased to `show(Object)`,
/// implemented as `show(a: Int)`. A parameter that erases to something else --
/// `Int` against `String` -- is a different method, not an override.
pub(crate) fn erases_to_object(st: &SymbolTable, ty: &Type) -> bool {
    matches!(
        ty,
        Type::TypeParam(_) | Type::TypeMember(_) | Type::Wildcard | Type::BoundedWildcard { .. }
    ) || jvm_desc(st, ty) == "Ljava/lang/Object;"
}

pub(crate) fn method_ret_from_sym(st: &SymbolTable, id: SymbolId) -> Type {
    match &st.get(id).ty {
        Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
        t => t.clone(),
    }
}

/// How an erasure bridge has to convert a value of type `from` to type `to`.
pub(crate) enum Adapt {
    None,
    Cast(String),
    Box(Type),
    Unbox(Type),
}

pub(crate) fn param_adapt(st: &SymbolTable, from: &Type, to: &Type) -> Adapt {
    // `Unit` is not unboxed out of an `Object` -- it *is* a reference,
    // `scala/runtime/BoxedUnit`. Treating it like the other primitives made a
    // bridge `pop` its argument and then call a `(BoxedUnit)` method with
    // nothing to hand it.
    if erases_to_boxed_unit(to) {
        return Adapt::Cast(BOXED_UNIT.to_string());
    }
    if erases_to_boxed_unit(from) {
        return Adapt::None;
    }
    // Both bottom types erase to their own classes. A bridge accepting
    // Object must checkcast before forwarding to a Null$ / Nothing$ slot.
    match to.widen_constant() {
        Type::Null => return Adapt::Cast("scala/runtime/Null$".into()),
        Type::Nothing => return Adapt::Cast(NOTHING_CLASS.to_string()),
        _ => {}
    }
    // Preserve main's bottom-value handling: a Nothing argument must not
    // fall through to primitive unboxing while forwarding a reference slot.
    if matches!(from.widen_constant(), Type::Nothing) {
        return Adapt::None;
    }
    if is_jvm_primitive(to) && !is_jvm_primitive(from) {
        Adapt::Unbox(to.clone())
    } else if is_jvm_primitive(from) && !is_jvm_primitive(to) {
        Adapt::Box(from.clone())
    } else {
        match checkcast_internal(st, to) {
            Some(cn) => Adapt::Cast(cn),
            None => Adapt::None,
        }
    }
}

pub(crate) fn emit_adapt(asm: &mut Assembler, adapt: &Adapt) {
    match adapt {
        Adapt::None => {}
        Adapt::Cast(cn) => asm.checkcast(cn),
        Adapt::Box(ty) => emit_box(asm, ty),
        Adapt::Unbox(ty) => emit_unbox(asm, ty),
    }
}

pub(crate) fn checkcast_internal(st: &SymbolTable, ty: &Type) -> Option<String> {
    match ty {
        Type::Null => Some("scala/runtime/Null$".into()),
        Type::Class { sym, .. } | Type::ModuleRef(sym) => Some(class_internal(st, *sym)),
        Type::String => Some("java/lang/String".into()),
        Type::Function { params, .. } => Some(format!("scala/Function{}", params.len())),
        // `def show(p: (A, B))` erases to `show(Tuple2)`; the bridge from
        // `show(Object)` has to checkcast, same as for any other class type.
        Type::Tuple(ts) if !ts.is_empty() => Some(format!("scala/Tuple{}", ts.len())),
        // An array's "internal name" in a `checkcast` is its descriptor
        // (`[I`), which is what the JVM spec asks for. Without this the bridge
        // `sizeOf(Object)I` for `def sizeOf(c: C[Int])` implemented at
        // `C = Array` handed an `Object` straight to `sizeOf([I)I`:
        // `VerifyError: Type 'java/lang/Object' is not assignable to '[I'`.
        Type::Array(_) => Some(jvm_desc(st, ty)),
        Type::Named { name, .. } => Some(name.replace('.', "/")),
        _ => None,
    }
}

pub(crate) fn method_desc_from_sym(st: &SymbolTable, id: SymbolId) -> String {
    let s = st.get(id);
    if s.name == "<init>" {
        if s.jvm_name.starts_with('(') && s.jvm_name.ends_with(")V") {
            return s.jvm_name.clone();
        }
        let params = match &s.ty {
            Type::Method { paramss, .. } => {
                let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
                if params.iter().any(|p| p.is_no_type() || p.is_error()) {
                    s.params.iter().map(|p| st.get(*p).ty.clone()).collect()
                } else {
                    params
                }
            }
            Type::Function { params, .. } => params.clone(),
            _ => s.params.iter().map(|p| st.get(*p).ty.clone()).collect(),
        };
        return jvm_method_desc(st, &params, &Type::Unit);
    }
    if s.jvm_name.starts_with('(') {
        return s.jvm_name.clone();
    }
    match &s.ty {
        Type::Method { paramss, ret } => {
            let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
            if params.iter().any(|p| p.is_no_type() || p.is_error()) {
                let params: Vec<Type> = s.params.iter().map(|p| st.get(*p).ty.clone()).collect();
                jvm_method_desc(st, &params, ret)
            } else {
                jvm_method_desc(st, &params, ret)
            }
        }
        Type::Function { params, ret } => jvm_method_desc(st, params, ret),
        _ => {
            let params: Vec<Type> = s.params.iter().map(|p| st.get(*p).ty.clone()).collect();
            jvm_method_desc(st, &params, &Type::Unit)
        }
    }
}

/// Scala inner classes take a hidden `$outer` as the first `<init>` argument.
/// Constructor symbols list only the source parameters, so descriptors from
/// `method_desc_from_sym` must be adjusted at emit time.
pub(crate) fn with_enclosing_outer_param(
    st: &SymbolTable,
    class_id: SymbolId,
    desc: &str,
) -> String {
    let Some(outer_ty) = outer_field_desc(st, class_id) else {
        return desc.to_string();
    };
    let Some(rest) = desc.strip_prefix('(') else {
        return desc.to_string();
    };
    if rest.starts_with(&outer_ty) {
        return desc.to_string();
    }
    format!("({outer_ty}{rest}")
}

pub(crate) fn ctor_desc(st: &SymbolTable, class_id: SymbolId, args: &[Tree]) -> String {
    if let Some(d) = java_ctor_desc(st, class_id, args.len()) {
        return d;
    }
    if let Some(id) = pick_init_sym(st, class_id, args) {
        return with_enclosing_outer_param(st, class_id, &method_desc_from_sym(st, id));
    }
    let mut d = String::from("(");
    if let Some(outer_ty) = outer_field_desc(st, class_id) {
        d.push_str(&outer_ty);
    }
    let fields = &st.get(class_id).ctor_fields;
    if !fields.is_empty() && fields.len() == args.len() {
        for f in fields {
            d.push_str(&jvm_desc(st, &st.get(*f).ty));
        }
    } else {
        for a in args {
            d.push_str(&jvm_desc(st, &a.ty));
        }
    }
    d.push_str(")V");
    d
}

pub(crate) fn pick_init_sym(
    st: &SymbolTable,
    class_id: SymbolId,
    args: &[Tree],
) -> Option<SymbolId> {
    if class_id.is_none() {
        return None;
    }
    let nargs = args.len();
    let inits: Vec<SymbolId> = st
        .lookup_member(class_id, "<init>")
        .into_iter()
        .filter(|&id| st.get(id).kind == SymKind::Method)
        .collect();
    let arity_ok = |id: SymbolId| {
        let s = st.get(id);
        match &s.ty {
            Type::Method { paramss, .. } => paramss.first().map(|p| p.len()).unwrap_or(0) == nargs,
            _ => s.params.len() == nargs,
        }
    };
    let typed: Vec<SymbolId> = inits.iter().copied().filter(|&id| arity_ok(id)).collect();
    if typed.len() == 1 {
        return typed.first().copied();
    }
    typed.into_iter().find(|&id| {
        let ps = match &st.get(id).ty {
            Type::Method { paramss, .. } => paramss.first().cloned().unwrap_or_default(),
            _ => st
                .get(id)
                .params
                .iter()
                .map(|p| st.get(*p).ty.clone())
                .collect(),
        };
        ps.iter().zip(args.iter()).all(|(p, a)| {
            p.is_no_type()
                || a.ty.is_no_type()
                || st.is_sub_type(&a.ty, p)
                || jvm_desc(st, p) == jvm_desc(st, &a.ty)
        })
    })
}

/// `(super internal name, `<init>` descriptor, explicit args, super class
/// symbol)`. The symbol is what tells the caller whether the superclass is a
/// nested class that needs an `$outer` argument ahead of the source ones.
/// The constructor's *declared* parameter types for `args`: `ctor_sym`'s own
/// method type when it is known (matches a Java or generic-erased signature
/// exactly, e.g. `AtomicReference[Int](x: Object)`), falling back to the
/// class's constructor fields, and finally to the arguments' own static
/// types when neither is available. Callers box a primitive argument
/// wherever this says the parameter is not itself a JVM primitive -- the
/// same check `gen_new` makes for an ordinary `new`.
pub(crate) fn ctor_param_tys(
    st: &SymbolTable,
    ctor_sym: SymbolId,
    class_id: SymbolId,
    args: &[Tree],
) -> Vec<Type> {
    if !ctor_sym.is_none() && st.get(ctor_sym).name == "<init>" {
        return match &st.get(ctor_sym).ty {
            Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
            _ => args.iter().map(|a| a.ty.clone()).collect(),
        };
    }
    if class_id.is_none() {
        return args.iter().map(|a| a.ty.clone()).collect();
    }
    let fields = st.get(class_id).ctor_fields.clone();
    if fields.is_empty() || fields.len() != args.len() {
        args.iter().map(|a| a.ty.clone()).collect()
    } else {
        fields.iter().map(|f| st.get(*f).ty.clone()).collect()
    }
}

pub(crate) fn parent_super_ctor(
    st: &SymbolTable,
    parents: &[Tree],
    super_name: &str,
) -> (String, String, Vec<Tree>, SymbolId, Vec<Type>) {
    for p in parents {
        if let TreeKind::Apply { args, .. } = &p.kind {
            if !p.sym.is_none() && st.get(p.sym).name == "<init>" {
                let cls = st.get(p.sym).owner;
                let owner = class_internal(st, cls);
                let desc = with_enclosing_outer_param(st, cls, &method_desc_from_sym(st, p.sym));
                let field_tys = ctor_param_tys(st, p.sym, cls, args);
                return (owner, desc, args.clone(), cls, field_tys);
            }
            if let Some(cls) = st.class_sym_of(&p.ty) {
                let owner = class_internal(st, cls);
                if owner == super_name || super_name == "java/lang/Object" {
                    let desc = ctor_desc(st, cls, args);
                    let field_tys = ctor_param_tys(st, SymbolId::NONE, cls, args);
                    return (owner, desc, args.clone(), cls, field_tys);
                }
            }
        }
        if !p.sym.is_none() && st.get(p.sym).name == "<init>" {
            let cls = st.get(p.sym).owner;
            let owner = class_internal(st, cls);
            let desc = with_enclosing_outer_param(st, cls, &method_desc_from_sym(st, p.sym));
            return (owner, desc, Vec::new(), cls, Vec::new());
        }
    }
    // No explicit parent constructor call. A nested superclass still needs its
    // `$outer`, so find the class the plain `extends A` names.
    for p in parents {
        if let Some(cls) = st.class_sym_of(&p.ty) {
            if class_internal(st, cls) == super_name {
                if let Some(outer_ty) = outer_field_desc(st, cls) {
                    return (
                        super_name.to_string(),
                        format!("({outer_ty})V"),
                        Vec::new(),
                        cls,
                        Vec::new(),
                    );
                }
                return (
                    super_name.to_string(),
                    "()V".into(),
                    Vec::new(),
                    cls,
                    Vec::new(),
                );
            }
        }
    }
    (
        super_name.to_string(),
        "()V".into(),
        Vec::new(),
        SymbolId::NONE,
        Vec::new(),
    )
}

/// Java `<init>` descriptors come from the classfile (`(Ljava/lang/Object;…)V`),
/// not from the Scala argument types (`String` would emit the wrong desc).
pub(crate) fn java_ctor_desc(st: &SymbolTable, class_id: SymbolId, nargs: usize) -> Option<String> {
    if class_id.is_none() || !st.get(class_id).flags.contains(Flags::JAVA) {
        return None;
    }
    st.lookup_member(class_id, "<init>")
        .into_iter()
        .find(|&id| {
            let s = st.get(id);
            s.kind == SymKind::Method && s.params.len() == nargs && s.jvm_name.starts_with('(')
        })
        .map(|id| st.get(id).jvm_name.clone())
}

pub(crate) fn enclosing_instance(st: &SymbolTable, class_id: SymbolId) -> Option<SymbolId> {
    if class_id.is_none() {
        return None;
    }
    // Static nested Java types (`Map$Entry`, `AbstractMap$SimpleEntry`) must
    // not get an enclosing `this` argument.
    if st.get(class_id).flags.contains(Flags::JAVA) {
        return None;
    }
    // `new T { … }` and local classes are owned by the method (or the `val`)
    // they appear in; the enclosing instance is the class around it.
    let mut owner = st.get(class_id).owner;
    while !owner.is_none() && matches!(st.get(owner).kind, SymKind::Method | SymKind::Term) {
        owner = st.get(owner).owner;
    }
    if owner.is_none() {
        return None;
    }
    let o = st.get(owner);
    if o.kind != SymKind::Class || o.flags.contains(Flags::MODULE) {
        // A member of a *non-static* `object` still has an enclosing instance:
        // in `class Outer { object P { object N } }` nsc types `N`'s `$outer`
        // as `Outer$P$`, because `P` itself is one instance per `Outer`.
        if (o.kind == SymKind::ModuleClass || o.flags.contains(Flags::MODULE))
            && member_module_outer(st, owner).is_some()
        {
            return Some(owner);
        }
        return None;
    }
    // A member class of a trait gets an `$outer` just like one of a class:
    // the trait is an interface, so its members are only reachable through an
    // instance (nsc passes the interface type as the first `<init>` argument).
    Some(owner)
}

/// The JVM type of `class_id`'s `$outer` field. nsc types it as the enclosing
/// class's *self* type, so a cake component (`trait C { self: P => class T }`)
/// stores a `P` and reaches `P`'s members without a cast. The self type is
/// only taken when it really is a subclass of the enclosing class, so the
/// field can always stand in for the enclosing instance itself.
pub(crate) fn outer_field_class(st: &SymbolTable, class_id: SymbolId) -> Option<SymbolId> {
    let owner = enclosing_instance(st, class_id)?;
    Some(self_repr_class(st, owner))
}

pub(crate) fn self_repr_class(st: &SymbolTable, owner: SymbolId) -> SymbolId {
    let Some(sty) = st.get(owner).self_type.clone() else {
        return owner;
    };
    let Some(s) = st.class_sym_of(&sty) else {
        return owner;
    };
    if s == owner || st.get(s).flags.contains(Flags::JAVA) || !is_owner_compatible(st, s, owner) {
        return owner;
    }
    s
}

pub(crate) fn outer_field_desc(st: &SymbolTable, class_id: SymbolId) -> Option<String> {
    outer_field_class(st, class_id).map(|o| format!("L{};", class_internal(st, o)))
}

/// SLS 5.1.2, `cls` first. The C3 merge itself lives in the typer
/// (`scala_rs_typer::linearize`) so that the `abstract override` grounding
/// check and the super accessors it drives cannot disagree about the order.
pub(crate) fn linearize(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    scala_rs_typer::linearize(st, cls)
}

/// The class a value of the `from` descriptor's result must be cast to for the
/// `to` descriptor's result, or `None` when the two agree or either side is
/// not a plain object reference.
pub(crate) fn narrowing_return_cast(from: &str, to: &str) -> Option<String> {
    let f = from.rsplit(')').next()?;
    let t = to.rsplit(')').next()?;
    if f == t || !t.starts_with('L') || !t.ends_with(';') || !f.starts_with('L') {
        return None;
    }
    Some(t[1..t.len() - 1].to_string())
}

/// nsc's expanded `super` accessor of a stackable trait: `p$q$T$$super$m`,
/// named after the trait's *internal* name and not its simple one. A class
/// real scalac compiles implements the name nsc expands to, so anything
/// shorter is an `AbstractMethodError` the moment the class is not ours.
pub(crate) fn super_accessor_name(st: &SymbolTable, trait_id: SymbolId, method: &str) -> String {
    let owner = class_internal(st, trait_id).replace('/', "$");
    format!("{owner}$$super${}", encode_method_name(method))
}

/// nsc's expanded outer accessor of a *trait*: `Main$Outer$T$$$outer`.
///
/// A trait compiles to an interface and so cannot hold the `$outer` field
/// itself. scalac declares this accessor abstractly on the interface and lets
/// every implementing class return its own enclosing instance through it — the
/// trait's own code (its `default` method bodies) then reads the
/// enclosing instance by calling it instead of by `getfield $outer`.
pub(crate) fn trait_outer_accessor_name(st: &SymbolTable, trait_id: SymbolId) -> String {
    format!("{}$$$outer", class_internal(st, trait_id).replace('/', "$"))
}

/// Read the `$outer` of `cur` off an instance already on the stack.
pub(crate) fn load_outer_of(asm: &mut Assembler, st: &SymbolTable, cur: SymbolId, held: SymbolId) {
    let owner = class_internal(st, cur);
    let desc = format!("L{};", class_internal(st, held));
    if is_interface_sym(st, cur) {
        asm.invokeinterface(
            &owner,
            &trait_outer_accessor_name(st, cur),
            &format!("(){desc}"),
        );
    } else {
        asm.getfield(&owner, "$outer", &desc);
    }
}

/// nsc's mixin setter for a trait `val`: `p$q$T$_setter_$v_$eq`. Naming it
/// after the owning trait is what lets two traits carry a `val` of the same
/// name, and what lets a class that `override val`s one implement the setter
/// as a no-op (see `emit_trait_val_accessors`).
pub(crate) fn trait_val_setter_name(st: &SymbolTable, trait_id: SymbolId, field: &str) -> String {
    let owner = class_internal(st, trait_id).replace('/', "$");
    format!("{owner}$_setter_${}_$eq", encode_method_name(field))
}

/// `java.lang.String.hashCode`, so a `case object`'s `hashCode` can be folded
/// to the same constant nsc folds it to.
pub(crate) fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// A `var`'s setter, trait member or not: nsc's plain `v_$eq`.
pub(crate) fn var_setter_name(field: &str) -> String {
    format!("{}_$eq", encode_method_name(field))
}

/// The setter `<Iface>.$init$` (and an assignment) goes through for a trait
/// member: a `var` has an ordinary public setter, a `val` only the mixin one.
pub(crate) fn trait_member_setter_name(
    st: &SymbolTable,
    trait_id: SymbolId,
    field: &str,
    mutable: bool,
) -> String {
    if mutable {
        var_setter_name(field)
    } else {
        trait_val_setter_name(st, trait_id, field)
    }
}

pub(crate) fn tree_contains_super(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::Super { .. } => true,
        TreeKind::Select { qual, .. } => tree_contains_super(qual),
        TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
            tree_contains_super(fun) || args.iter().any(tree_contains_super)
        }
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
            tree_contains_super(fun)
        }
        TreeKind::Block { stats, expr } => {
            stats.iter().any(tree_contains_super) || tree_contains_super(expr)
        }
        TreeKind::If { cond, thenp, elsep } => {
            tree_contains_super(cond) || tree_contains_super(thenp) || tree_contains_super(elsep)
        }
        TreeKind::Assign { lhs, rhs } => tree_contains_super(lhs) || tree_contains_super(rhs),
        TreeKind::ValDef { rhs, .. } => tree_contains_super(rhs),
        TreeKind::Function { body, .. } => tree_contains_super(body),
        TreeKind::Match { selector, cases } => {
            tree_contains_super(selector)
                || cases.iter().any(|c| {
                    tree_contains_super(&c.pat)
                        || tree_contains_super(&c.guard)
                        || tree_contains_super(&c.body)
                })
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            tree_contains_super(block)
                || catches.iter().any(|c| tree_contains_super(&c.body))
                || tree_contains_super(finalizer)
        }
        _ => false,
    }
}

pub(crate) fn needs_super_accessor(def: &Tree) -> bool {
    match &def.kind {
        TreeKind::DefDef {
            name, mods, rhs, ..
        } => {
            name != "<init>"
                && name != "<clinit>"
                && !rhs.is_empty()
                && (mods.flags.contains(Flags::OVERRIDE) || tree_contains_super(rhs))
        }
        _ => false,
    }
}

pub(crate) fn is_star_pat(pat: &Tree) -> bool {
    match &pat.kind {
        TreeKind::Star { .. } => true,
        TreeKind::Bind { body, .. } => is_star_pat(body),
        TreeKind::Typed { expr, .. } => is_star_pat(expr),
        _ => false,
    }
}

/// A `lazy val` inherited from a trait that arrived as a class file.
pub(crate) struct BinaryLazyVal {
    pub(crate) name: String,
    pub(crate) ty: Type,
    /// The interface whose `d$` static holds the initialiser.
    pub(crate) owner: SymbolId,
}

/// Where a `lazy val` accessor gets the value it caches.
pub(crate) enum LazyInit {
    /// The initialiser as written, in this run's own trees.
    Rhs(Box<Tree>),
    /// `<Iface>.d$(this)`: a trait read from a class file put its initialiser
    /// in a `default` method, and the static beside it is the entry point.
    TraitStatic {
        iface: String,
        static_name: String,
        static_desc: String,
    },
}

/// A trait's `lazy val` as the method nsc compiles it to.
///
/// The interface holds the *initialiser*, not the caching: nsc emits a
/// `default d()` with the right-hand side in it and the usual `d$` static
/// beside it, and the implementing class's `d$lzycompute` calls that static
/// under its own `bitmap$0`. The accessor stays public even for a `private
/// lazy val` (under the expanded name), because a `private static` of one
/// class file is not callable from another.
pub(crate) fn lazy_val_as_def(vd: &Tree) -> Tree {
    let TreeKind::ValDef {
        mods,
        name,
        tpt,
        rhs,
    } = &vd.kind
    else {
        return vd.clone();
    };
    let mut mods = mods.clone();
    mods.flags.set(Flags::PRIVATE, false);
    mods.flags.set(Flags::LOCAL, false);
    mods.private_within = None;
    Tree {
        kind: TreeKind::DefDef {
            mods,
            name: name.clone(),
            tparams: Vec::new(),
            vparamss: Vec::new(),
            tpt: tpt.clone(),
            rhs: rhs.clone(),
        },
        ..vd.clone()
    }
}

pub(crate) fn val_tree_ty(st: &SymbolTable, vd: &Tree) -> Type {
    if !vd.ty.is_no_type() {
        vd.ty.clone()
    } else if !vd.sym.is_none() {
        st.get(vd.sym).ty.clone()
    } else {
        Type::Any
    }
}

pub(crate) fn is_trait_owned_term(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    let s = st.get(id);
    if s.kind != SymKind::Term || s.flags.contains(Flags::PARAM) {
        return false;
    }
    let o = s.owner;
    !o.is_none() && is_interface_sym(st, o) && !is_module_class(st, o)
}

/// A class's `val` / `var` member is read through its accessor, not its field.
///
/// scala-rs emits such a field public and used to read it with `getfield` on
/// the class that *declares* it. A subclass's `override val` has a slot of its
/// own, so the override was invisible (`(new A: P).pre` gave the parent's
/// value), and an `abstract val` read a slot nothing ever wrote (`null`).
/// nsc calls the accessor for every member value that is not `private`, and
/// virtual dispatch then lands on whichever class actually holds the value.
///
/// Constructor parameters (`case class C(name: String)`) keep the direct read:
/// they are the hot path, and the synthesized members that back them
/// (`equals`, `copy`, `productElement`) read the field too.
///
/// A member of a **separately compiled** class is in the same position for a
/// different reason -- scalac made its field `private` -- and says so with
/// [`scala_rs_typer::Symbol::via_accessor`], which the class file's own method
/// list decided.
pub(crate) fn reads_via_accessor(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    let s = st.get(id);
    if s.via_accessor {
        return true;
    }
    if s.kind != SymKind::Term
        || s.flags.contains(Flags::PARAM)
        || s.flags.contains(Flags::STATIC)
        || s.flags.contains(Flags::PRIVATE)
        || !s.jvm_name.is_empty()
    {
        return false;
    }
    let o = s.owner;
    // Only classes compiled in this run: those are the ones scala-rs emits an
    // accessor for. A prelude or classfile symbol says how to reach it in
    // `jvm_name` (empty means "read the field", which is right for the private
    // runtime's `Tuple2._1`).
    !o.is_none()
        && st.get(o).is_class_like()
        && !is_interface_sym(st, o)
        && st.source_classes.contains(&o)
}

pub(crate) fn trait_static_desc(iface: &str, inst_desc: &str) -> String {
    let rest = inst_desc.strip_prefix('(').unwrap_or(inst_desc);
    format!("(L{iface};{rest}")
}

/// nsc 2.13's name for the `static` forwarder that sits on the interface
/// beside every concrete trait method: `m$`, taking the receiver as its first
/// parameter. Every mixin forwarder and every `super` call into a trait goes
/// through it, because `invokespecial` on a default method would require the
/// interface to be a *direct* superinterface of the caller and it usually is
/// not. Idempotent under `encode_method_name` (`<` is `$less`, so the
/// forwarder is `$less$`), which is what the assembler applies again.
pub(crate) fn trait_static_name(name: &str) -> String {
    format!("{}$", encode_method_name(name))
}

pub(crate) fn type_jvm_name(st: &SymbolTable, ty: &Type) -> String {
    match ty {
        Type::Class { sym, .. } | Type::ModuleRef(sym) => class_internal(st, *sym),
        Type::Named { name, .. } => name.replace('.', "/"),
        Type::String => "java/lang/String".into(),
        // `case i: Int` tests the box: a boxed scrutinee is what it holds.
        Type::Int => "java/lang/Integer".into(),
        Type::Long => "java/lang/Long".into(),
        Type::Double => "java/lang/Double".into(),
        Type::Float => "java/lang/Float".into(),
        Type::Short => "java/lang/Short".into(),
        Type::Byte => "java/lang/Byte".into(),
        Type::Char => "java/lang/Character".into(),
        Type::Boolean => "java/lang/Boolean".into(),
        _ => "java/lang/Object".into(),
    }
}

pub(crate) fn is_interface_sym(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::INTERFACE)
}

/// `Any` / `AnyRef` / `AnyVal` / `Object` — the top of every hierarchy, and
/// the one class parent an interface really does reach on the JVM.
pub(crate) fn is_top_class(st: &SymbolTable, id: SymbolId) -> bool {
    matches!(
        st.get(id).name.as_str(),
        "Any" | "AnyRef" | "AnyVal" | "Object"
    )
}

/// True when `owner` is `current` or a parent in the extends/with graph.
/// Self types are *not* walked: a trait `self: Foo =>` must checkcast `$this`.
///
/// Nor is a trait's *superclass* (SLS 5.3.3 `trait T extends C`): that parent
/// is a constraint on the classes that may mix `T` in, and `emit_class` drops
/// it from the interface's class file, so `LT;` is not assignable to `LC;` on
/// the JVM. Walking it made a trait body read an inherited `C` member off
/// `$this` with no cast and the verifier rejected the method
/// (`Type 'T' is not assignable to 'C'`).
pub(crate) fn is_owner_compatible(st: &SymbolTable, current: SymbolId, owner: SymbolId) -> bool {
    if owner.is_none() || current == owner {
        return true;
    }
    let mut work = vec![current];
    let mut seen = HashSet::new();
    while let Some(id) = work.pop() {
        if !seen.insert(id.0) {
            continue;
        }
        if id == owner {
            return true;
        }
        let from_iface = is_interface_sym(st, id);
        for p in &st.get(id).parents {
            if let Some(ps) = st.class_sym_of(p) {
                if from_iface && !is_interface_sym(st, ps) && !is_top_class(st, ps) {
                    continue;
                }
                work.push(ps);
            }
        }
    }
    false
}

/// Like `is_owner_compatible`, but a *trait*'s class parent counts.
///
/// The JVM interface a trait becomes does not extend that class, which is why
/// `is_owner_compatible` refuses to follow the edge -- a call through the
/// interface cannot assume it. `this` is different: every instance of
/// `trait U extends B` really is a `B`, so `B`'s members are read off `this`
/// with a `checkcast`, which is what nsc emits. Without this,
/// `trait Comp { class B(val table: String); object B { trait U extends B {
/// … table … } } }` walked out to `U`'s `$outer` (the `B` *module*) and on to
/// `Comp`, then cast that to `B`: slick's
/// `TableDDLBuilder.UniqueIndexAsConstraint` threw `ClassCastException:
/// H2Profile$ cannot be cast to …$TableDDLBuilder` on its first line.
pub(crate) fn self_reaches_owner(st: &SymbolTable, current: SymbolId, owner: SymbolId) -> bool {
    if owner.is_none() || current == owner {
        return true;
    }
    let mut work = vec![current];
    let mut seen = HashSet::new();
    while let Some(id) = work.pop() {
        if !seen.insert(id.0) {
            continue;
        }
        if id == owner {
            return true;
        }
        for p in &st.get(id).parents {
            if let Some(ps) = st.class_sym_of(p) {
                work.push(ps);
            }
        }
    }
    false
}

/// The `$outer` chain out of `from` reaches an instance that owns `owner`'s
/// members — i.e. the member is available without reading `from`'s own `this`.
pub(crate) fn outer_chain_reaches_owner(st: &SymbolTable, from: SymbolId, owner: SymbolId) -> bool {
    let mut cur = from;
    let mut seen = HashSet::new();
    while let Some(o) = enclosing_instance(st, cur) {
        if !seen.insert(o.0) {
            return false;
        }
        if self_reaches_owner(st, o, owner) {
            return true;
        }
        cur = o;
    }
    false
}

/// Push the instance that owns `owner`'s members: `this`, or the `$outer`
/// chain of the class being emitted when the member lives further out.
/// `cur` is the class we are lexically inside (it decides the next hop),
/// `held` the static type on the stack — the two differ when a trait's
/// `$outer` is typed as the trait's self type.
pub(crate) fn load_owner_instance(asm: &mut Assembler, ctx: &EmitCtx, owner: SymbolId) {
    let hops = !ctx.class_sym.is_none()
        && !owner.is_none()
        && (!self_reaches_owner(ctx.st, ctx.class_sym, owner)
            // In the pre-super part of an `<init>` slot 0 is
            // `uninitializedThis`, which JVMS §4.10.1.9 lets `putfield` take
            // and nothing else -- no `getfield`, no `invokevirtual`. A
            // reference there is also *written* outside the template: the
            // arguments of `new P(rs) { … }` belong to the enclosing class, so
            // `rs` means the enclosing instance's `rs` and not the field of the
            // object being built, which has not been assigned yet. So walk out
            // to the constructor's `$outer` parameter whenever the enclosing
            // instance can supply the member, *even though* the class being
            // constructed inherits it as well.
            //
            // slick's `PositionedResult.view` is exactly this shape
            // (`new PositionedResult(rs) { … }` inside `PositionedResult`), and
            // reading `rs` off `this` had the JVM refuse
            // `slick.jdbc.PositionedResult$$anon$507` outright: `VerifyError:
            // Bad type on operand stack … Type uninitializedThis … is not
            // assignable to 'slick/jdbc/PositionedResult'`.
            || (ctx.presuper_outer.is_some()
                && outer_chain_reaches_owner(ctx.st, ctx.class_sym, owner)));
    let (mut cur, mut held) = start_outer_walk(asm, ctx, hops);
    while !cur.is_none() && !owner.is_none() && !self_reaches_owner(ctx.st, held, owner) {
        let Some(o) = enclosing_instance(ctx.st, cur) else {
            break;
        };
        let f = outer_field_class(ctx.st, cur).unwrap_or(o);
        load_outer_of(asm, ctx.st, cur, f);
        cur = o;
        held = f;
    }
    if !is_owner_compatible(ctx.st, held, owner) {
        let kind = ctx.st.get(owner).kind;
        if matches!(kind, SymKind::Class | SymKind::ModuleClass) || is_interface_sym(ctx.st, owner)
        {
            asm.checkcast(&class_internal(ctx.st, owner));
        }
    }
}

/// A template's self alias denotes *that* template's own `this`. A class
/// written inside it may also be a *subclass* of it, and then `this` conforms
/// to the alias's class while being the wrong object: in slick's
/// `def ++(other: DDL) = new DDL { … self.createPhase1 … }` the alias means the
/// enclosing `DDL`, so reading it off `this` called the override back and
/// looped. Walk out to the class that owns the alias by identity, not by
/// conformance.
pub(crate) fn load_self_alias_instance(asm: &mut Assembler, ctx: &EmitCtx, owner: SymbolId) {
    if ctx.class_sym == owner || !outer_chain_reaches_exactly(ctx.st, ctx.class_sym, owner) {
        load_owner_instance(asm, ctx, owner);
        return;
    }
    let (mut cur, mut held) = start_outer_walk(asm, ctx, ctx.class_sym != owner);
    while cur != owner {
        let Some(o) = enclosing_instance(ctx.st, cur) else {
            break;
        };
        let f = outer_field_class(ctx.st, cur).unwrap_or(o);
        load_outer_of(asm, ctx.st, cur, f);
        cur = o;
        held = f;
    }
    if !is_owner_compatible(ctx.st, held, owner) {
        let kind = ctx.st.get(owner).kind;
        if matches!(kind, SymKind::Class | SymKind::ModuleClass) || is_interface_sym(ctx.st, owner)
        {
            asm.checkcast(&class_internal(ctx.st, owner));
        }
    }
}

/// A `private[this]` member denotes *that* instance, exactly like a self
/// alias: a class nested inside the owner may also be a *subclass* of it, and
/// then `this` conforms to the owner while being the wrong object. slick's
/// `SynchronousDatabaseAction` reaches `private[this] def superZip` from an
/// anonymous `SynchronousDatabaseAction.Fused`, and calling it on `this` ran
/// the fused action's own state instead of the enclosing one's. So the
/// receiver has to be walked out by identity (`load_self_alias_instance`).
pub(crate) fn is_private_this(st: &SymbolTable, id: SymbolId) -> bool {
    let f = st.get(id).flags;
    f.contains(Flags::PRIVATE) && f.contains(Flags::LOCAL)
}

/// `outer_chain_reaches` by identity: the `$outer` chain actually arrives at
/// `owner` itself, not merely at something that conforms to it.
pub(crate) fn outer_chain_reaches_exactly(
    st: &SymbolTable,
    from: SymbolId,
    owner: SymbolId,
) -> bool {
    let mut cur = from;
    loop {
        if cur == owner {
            return true;
        }
        let Some(o) = enclosing_instance(st, cur) else {
            return false;
        };
        cur = o;
    }
}

/// True when `load_owner_instance` can actually reach `owner` — either `this`
/// or some link of the `$outer` chain conforms to it. When it cannot, the
/// caller must look for an enclosing object instead of emitting a cast that
/// would fail at run time.
pub(crate) fn outer_chain_reaches(st: &SymbolTable, from: SymbolId, owner: SymbolId) -> bool {
    let mut cur = from;
    let mut held = from;
    loop {
        if is_owner_compatible(st, held, owner) {
            return true;
        }
        let Some(o) = enclosing_instance(st, cur) else {
            return false;
        };
        held = outer_field_class(st, cur).unwrap_or(o);
        cur = o;
    }
}

/// The nearest enclosing object whose instance can serve as `owner`'s
/// `$outer`: `object DB2Profile extends … { class T extends Table }` hands
/// `DB2Profile$.MODULE$` to `Table`'s constructor, exactly as nsc does.
pub(crate) fn enclosing_module_for(
    st: &SymbolTable,
    from: SymbolId,
    owner: SymbolId,
) -> Option<SymbolId> {
    if owner.is_none() {
        return None;
    }
    let mut cur = from;
    while !cur.is_none() {
        let s = st.get(cur);
        if (s.kind == SymKind::ModuleClass || s.flags.contains(Flags::MODULE))
            && is_owner_compatible(st, cur, owner)
        {
            return Some(cur);
        }
        cur = s.owner;
    }
    None
}

/// The nearest enclosing module class that conforms to `owner`, judged by the
/// linearisation rather than by [`is_owner_compatible`].
///
/// `is_owner_compatible` deliberately stops at a trait's non-interface parent,
/// because a JVM interface type cannot reach a superclass. For deciding *which
/// instance* to hand back that stop is wrong: `trait Baz extends Foo; object
/// Biz extends Baz` really is a `Foo`, and `Foo` is on `Biz$`'s linearisation.
pub(crate) fn enclosing_module_conforming(
    st: &SymbolTable,
    from: SymbolId,
    owner: SymbolId,
) -> Option<SymbolId> {
    if owner.is_none() {
        return None;
    }
    let mut cur = from;
    while !cur.is_none() {
        let s = st.get(cur);
        if (s.kind == SymKind::ModuleClass || s.flags.contains(Flags::MODULE))
            && (is_owner_compatible(st, cur, owner) || linearize(st, cur).contains(&owner))
        {
            return Some(cur);
        }
        cur = s.owner;
    }
    None
}

/// Push the instance a nested class's `$outer` parameter wants at a `new` or
/// at a super-constructor call.
pub(crate) fn load_outer_arg(asm: &mut Assembler, ctx: &EmitCtx, owner: SymbolId) {
    if !outer_chain_reaches(ctx.st, ctx.class_sym, owner) {
        if let Some(m) = enclosing_module_for(ctx.st, ctx.class_sym, owner) {
            load_module_instance(asm, ctx, m);
            if !is_owner_compatible(ctx.st, m, owner) {
                asm.checkcast(&class_internal(ctx.st, owner));
            }
            return;
        }
    }
    load_owner_instance(asm, ctx, owner);
}

pub(crate) fn maybe_checkcast_owner(asm: &mut Assembler, ctx: &EmitCtx, owner: SymbolId) {
    if is_owner_compatible(ctx.st, ctx.class_sym, owner) {
        return;
    }
    let kind = ctx.st.get(owner).kind;
    if matches!(kind, SymKind::Class | SymKind::ModuleClass) || is_interface_sym(ctx.st, owner) {
        let jn = class_internal(ctx.st, owner);
        asm.checkcast(&jn);
    }
}

pub(crate) fn checkcast_refined_receiver(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    qual_ty: &Type,
    method_id: SymbolId,
) {
    if method_id.is_none() {
        return;
    }
    if !matches!(qual_ty, Type::Refined { .. }) {
        return;
    }
    let owner = ctx.st.get(method_id).owner;
    if owner.is_none() {
        return;
    }
    let jn = class_internal(ctx.st, owner);
    if jn.is_empty() || jn == "java/lang/Object" {
        return;
    }
    asm.checkcast(&jn);
}

/// Captured locals are stored as `Object`. Erasure must checkcast before
/// `invokevirtual` / `invokeinterface` against a more specific owner
/// (`new Breaks` captured into `breakable { b.break() }`).
pub(crate) fn checkcast_erased_method_receiver(asm: &mut Assembler, ctx: &EmitCtx, fun: &Tree) {
    if fun.sym.is_none() || fun_is_super(fun) {
        return;
    }
    checkcast_method_receiver_sym(asm, ctx, fun.sym, false);
}

/// The same, for a receiver already on the stack under a call this function's
/// caller has decided is not a `super` one. Split out so the paren-less
/// `Select` path can take the step too: a receiver whose erased type does not
/// reach the method's declaring class is a `VerifyError` whether or not the
/// call has an argument list. `type TypeName >: Null <: TypeNameApi with
/// Name` erases to `TypeNameApi`, and `toTermName` is declared by `NameApi`
/// (which the second half of the bound leads to), so slick's
/// `ShapedValue.mapToImpl` -- `rSym.name.toTermName` -- threw the whole method
/// out. See `Check::members_through_compound_bound`.
/// `require_known` is what the paren-less caller passes: it emits a cast only
/// when the assembler can see what is on the stack *and* the verifier will
/// reject it. The `Apply` path knows the receiver is there and casts even when
/// the model has lost track; the `Select` path is also reached while emitting
/// synthetic forwarders, where the top of the stack is an argument (a `Query
/// .take(I)` forwarder had an `int` there) and casting it is a `VerifyError`
/// of its own making.
pub(crate) fn checkcast_method_receiver_sym(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    id: SymbolId,
    require_known: bool,
) {
    if require_known && asm.top_object().is_none() {
        return;
    }
    let s = ctx.st.get(id);
    if s.kind != SymKind::Method || s.flags.contains(Flags::STATIC) {
        return;
    }
    if s.name == "<init>" {
        return;
    }
    if is_module_class(ctx.st, s.owner) {
        return;
    }
    if ctx.st.is_value_class(s.owner) {
        return;
    }
    // The call names the declaring class when the owner's own class file does
    // not reach the method, so the receiver has to be cast to that class and
    // not to the owner (`Symbol::declaring_class`).
    let jn = if s.declaring_class.is_empty() {
        class_internal(ctx.st, s.owner)
    } else {
        s.declaring_class.clone()
    };
    if jn.is_empty() || jn == "java/lang/Object" || jn.starts_with('[') {
        return;
    }
    // The receiver is already on the stack, and the assembler models what the
    // verifier will see there -- the same model the StackMapTable is written
    // from. When the verifier will accept the `invoke*` on that value as it
    // stands, this cast is three wasted bytes and nsc emits none: `this.m()`
    // inside `C` is `aload_0; invokevirtual C.m`, four bytes to our seven.
    //
    // That is not only noise. `scala/test/files/run/t10594` came out 43%
    // larger than nsc's, and since `Method too large` became a diagnostic
    // rather than a silently truncated class, the difference decides whether a
    // method nsc accepts is rejected here. The 64 KB ceiling of JVMS 4.7.3 was
    // landing at about 57% of the source size nsc reaches.
    //
    // A `checkcast` never takes part in method resolution -- the `Methodref`
    // does, and `Symbol::declaring_class` above has already put the right
    // class in it -- so dropping one is safe exactly when the verifier does
    // not need it.
    if let Some(top) = asm.top_object() {
        if verifier_accepts_receiver(ctx.st, top, &jn) {
            return;
        }
    }
    asm.checkcast(&jn);
}

/// Whether JVMS 4.10.1.9 will accept an `invoke*` whose `Methodref` names
/// `to` on a receiver the assembler tracks as JVM class `from`.
///
/// Two things have to hold, and the second is not a detail.
///
/// `from` has to reach `to` in the symbol table at all, which is what
/// `jvm_assignable` answers -- `false` for either name it cannot resolve, so
/// an unknown class keeps its cast. A redundant `checkcast` is a no-op; a
/// missing one is a `VerifyError`.
///
/// And when `to` is a *class*, `from` has to be one too. The symbol table's
/// base type sequence is the **Scala** hierarchy, and a Scala trait may extend
/// a class -- a hop the bytecode cannot make, because the trait compiles to an
/// interface. `scala.reflect.api.JavaUniverse` is the standing example
/// (`Symbol::declaring_class` documents it from the other side): the pickle
/// says it extends the abstract class `Universe`, and its class file declares
/// `interfaces: 0` and no superclass but `Object`. Trusting the Scala answer
/// there dropped the cast in front of `Universe.TypeTag` and nine `run` tests
/// of the scala/scala corpus died with `VerifyError: Bad type on operand
/// stack -- Type 'scala/reflect/api/JavaUniverse' is not assignable to
/// 'scala/reflect/api/Universe'`.
///
/// The converse hop needs no guard: JVMS 4.10.1.2 makes *every* reference
/// assignable to an interface type, so an interface `to` is accepted whatever
/// is on the stack, and only the `Methodref` decides what gets called.
pub(crate) fn verifier_accepts_receiver(st: &SymbolTable, from: &str, to: &str) -> bool {
    if !jvm_assignable(st, from, to) {
        return false;
    }
    is_interface_jvm(st, to) || !is_interface_jvm(st, from)
}

pub(crate) fn is_module_class(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    s.kind == SymKind::ModuleClass || s.kind == SymKind::Module || s.flags.contains(Flags::MODULE)
}

pub(crate) fn module_class_id(st: &SymbolTable, id: SymbolId) -> SymbolId {
    match st.get(id).ty {
        Type::ModuleRef(c) => c,
        _ => id,
    }
}

/// The template an `object` written *inside a class or trait* belongs to.
///
/// Such an object is not a static singleton: there is one instance per
/// enclosing instance. nsc gives it an `$outer` field and an `<init>` taking
/// the enclosing instance, and puts a lazily initialised `<name>$module` field
/// plus a `<name>()` accessor on the enclosing template — verified against
/// `javap -v -p -c` of scalac 2.13.16 output for `class Outer { object P }`.
/// A top-level `object`, and one nested in another `object`, keeps `MODULE$`.
///
/// The owner test is deliberately *direct*: an `object` written inside a
/// method is owned by the method, and nsc compiles those with a per-call
/// `scala.runtime.LazyRef` instead. That shape is not implemented; the typer's
/// `check_local_objects` diagnoses the ones that would need it.
pub(crate) fn member_module_outer(st: &SymbolTable, id: SymbolId) -> Option<SymbolId> {
    if id.is_none() {
        return None;
    }
    let cls = module_class_id(st, id);
    let s = st.get(cls);
    if !(s.kind == SymKind::ModuleClass || s.flags.contains(Flags::MODULE))
        || s.flags.contains(Flags::JAVA)
    {
        return None;
    }
    let owner = s.owner;
    if owner.is_none() {
        return None;
    }
    let o = st.get(owner);
    if o.flags.contains(Flags::JAVA) {
        return None;
    }
    if o.kind == SymKind::ModuleClass || o.flags.contains(Flags::MODULE) {
        // `class Outer { object P { object N } }`: `P` is not static, so
        // neither is `N` — scalac gives it a `$outer` of type `Outer$P$`.
        return member_module_outer(st, owner).map(|_| owner);
    }
    if o.kind != SymKind::Class {
        return None;
    }
    Some(owner)
}

/// Name of the accessor the enclosing template exposes for a member `object`
/// (`P` for `Main$Outer$P$`), and of the field behind it (`P$module`).
pub(crate) fn module_accessor_name(st: &SymbolTable, module_cls: SymbolId) -> String {
    strip_module_dollar(&st.get(module_cls).name)
}

pub(crate) fn module_field_name(st: &SymbolTable, module_cls: SymbolId) -> String {
    format!("{}$module", module_accessor_name(st, module_cls))
}

pub(crate) fn module_accessor_desc(st: &SymbolTable, module_cls: SymbolId) -> String {
    format!("()L{};", class_internal(st, module_cls))
}

/// Push the single instance of `module_cls` onto the stack: `MODULE$` for a
/// static singleton, or `<enclosing instance>.<name>()` for a member `object`.
pub(crate) fn load_module_instance(asm: &mut Assembler, ctx: &EmitCtx, module_cls: SymbolId) {
    let jvm = class_internal(ctx.st, module_cls);
    let Some(outer) = member_module_outer(ctx.st, module_cls) else {
        asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
        return;
    };
    // Inside the object itself — or inside a class nested in it — the single
    // instance is already `this`, or one hop along the `$outer` chain.
    if outer_chain_reaches(ctx.st, ctx.class_sym, module_cls) {
        load_owner_instance(asm, ctx, module_cls);
        return;
    }
    load_owner_instance(asm, ctx, outer);
    invoke_module_accessor(asm, ctx.st, outer, module_cls);
}

/// `<outer>.<name>()` on an enclosing instance that is already on the stack.
/// A trait's accessor is an interface method, a class's a virtual one.
pub(crate) fn invoke_module_accessor(
    asm: &mut Assembler,
    st: &SymbolTable,
    outer: SymbolId,
    module_cls: SymbolId,
) {
    let owner = class_internal(st, outer);
    let name = module_accessor_name(st, module_cls);
    let desc = module_accessor_desc(st, module_cls);
    if is_interface_sym(st, outer) {
        asm.invokeinterface(&owner, &name, &desc);
    } else {
        asm.invokevirtual(&owner, &name, &desc);
    }
}

pub(crate) fn strip_module_dollar(name: &str) -> String {
    if let Some(rest) = name.strip_suffix('$') {
        rest.to_string()
    } else {
        name.to_string()
    }
}

pub(crate) fn split_parents(st: &SymbolTable, parents: &[Tree]) -> (String, Vec<String>) {
    let mut super_name = "java/lang/Object".to_string();
    let mut ifaces = Vec::new();
    let mut found_class = false;
    for p in parents {
        // `trait Mono extends (Int => String)` really is a `scala.Function1`
        // on the JVM (nsc emits it in the interface list), so a parent written
        // as a structural function has to be read back as the class.
        let pty = st
            .function_class_form(&p.ty)
            .unwrap_or_else(|| p.ty.clone());
        let id = st
            .class_sym_of(&pty)
            .or_else(|| if p.sym.is_none() { None } else { Some(p.sym) });
        let Some(id) = id else {
            continue;
        };
        let s = st.get(id);
        let jvm = class_internal(st, id);
        if jvm == "java/lang/Object"
            || s.name == "AnyRef"
            || s.name == "Any"
            || s.name == "AnyVal"
            || s.name == "Object"
        {
            continue;
        }
        if is_interface_sym(st, id) {
            ifaces.push(jvm);
        } else if !found_class {
            super_name = jvm;
            found_class = true;
        } else {
            ifaces.push(jvm);
        }
    }
    if !found_class {
        // SLS 5.1.2: the superclass of a template is the superclass of its
        // linearisation, which a *trait* parent can supply --
        // `trait Baz extends Foo; object Biz extends Baz` is
        // `Biz$ extends Foo implements Baz` in nsc's own output. Emitting
        // `extends java/lang/Object` there left every member `Foo` declares
        // unreachable from `Biz$`, and an inner trait of `Foo` mixed into
        // something nested in `Biz` could not hand back a `Foo` at all.
        if let Some(inherited) = inherited_super_class(st, parents) {
            super_name = inherited;
        }
    }
    (super_name, ifaces)
}

/// The class a trait parent brings in as the template's superclass, if any.
///
/// Only a parent that needs no enclosing instance qualifies: the constructor
/// emitted for the template calls `super.<init>()V`, and a nested superclass
/// would want its `$outer` first. Such a template keeps the
/// `java/lang/Object` superclass it has today rather than getting a call that
/// cannot link.
pub(crate) fn inherited_super_class(st: &SymbolTable, parents: &[Tree]) -> Option<String> {
    let mut work: Vec<SymbolId> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    for p in parents {
        let pty = st
            .function_class_form(&p.ty)
            .unwrap_or_else(|| p.ty.clone());
        if let Some(id) = st
            .class_sym_of(&pty)
            .or_else(|| (!p.sym.is_none()).then_some(p.sym))
        {
            if is_interface_sym(st, id) {
                work.push(id);
            }
        }
    }
    // Breadth-first, so the nearest parent wins the way the linearisation's
    // first class does.
    let mut i = 0;
    while i < work.len() {
        let id = work[i];
        i += 1;
        if !seen.insert(id.0) {
            continue;
        }
        for parent in &st.get(id).parents {
            let Some(ps) = st.class_sym_of(parent) else {
                continue;
            };
            if is_top_class(st, ps) {
                continue;
            }
            if is_interface_sym(st, ps) {
                work.push(ps);
            } else if outer_field_desc(st, ps).is_none() {
                return Some(class_internal(st, ps));
            }
        }
    }
    None
}

pub(crate) fn class_extends_named(st: &SymbolTable, id: SymbolId, name: &str) -> bool {
    if id.is_none() {
        return false;
    }
    if st.get(id).name == name {
        return true;
    }
    let mut work = st.get(id).parents.clone();
    let mut seen = HashSet::new();
    seen.insert(id.0);
    while let Some(p) = work.pop() {
        let Some(pid) = st.class_sym_of(&p) else {
            continue;
        };
        if !seen.insert(pid.0) {
            continue;
        }
        if st.get(pid).name == name {
            return true;
        }
        work.extend(st.get(pid).parents.clone());
    }
    false
}

pub(crate) fn extends_delayed_init(st: &SymbolTable, id: SymbolId) -> bool {
    class_extends_named(st, id, "DelayedInit") || class_extends_named(st, id, "App")
}

pub(crate) fn extends_app(st: &SymbolTable, id: SymbolId) -> bool {
    class_extends_named(st, id, "App")
}

/// A bare expression statement in a template body — anything that is not a
/// definition, an import or empty.
///
/// SLS 5.1: such a statement is part of the template's *initializer*. For a
/// class it runs inside the primary constructor, for a trait inside `$init$`,
/// for an `object` inside the module constructor — in every case interleaved
/// with the `val`/`var` initializers in declaration order. Dropping them
/// (which this backend used to do, since the constructor emitters filtered the
/// body down to `ValDef`s) compiles silently and then simply does not run the
/// code.
pub(crate) fn is_template_stat(t: &Tree) -> bool {
    !matches!(
        t.kind,
        TreeKind::DefDef { .. }
            | TreeKind::TypeDef { .. }
            | TreeKind::ClassDef { .. }
            | TreeKind::ModuleDef { .. }
            | TreeKind::ValDef { .. }
            | TreeKind::Import { .. }
            | TreeKind::PackageDef { .. }
            | TreeKind::Empty
    )
}

/// The template body entries the constructor has to run, in source order: the
/// `val`/`var` initializers plus the bare statements between them.
pub(crate) fn template_init_stats(body: &[Tree]) -> Vec<&Tree> {
    body.iter()
        .filter(|t| matches!(t.kind, TreeKind::ValDef { .. }) || is_template_stat(t))
        .collect()
}

pub(crate) fn is_delayed_ctor_stat(t: &Tree) -> bool {
    match &t.kind {
        TreeKind::DefDef { .. }
        | TreeKind::TypeDef { .. }
        | TreeKind::ClassDef { .. }
        | TreeKind::ModuleDef { .. }
        | TreeKind::Import { .. }
        | TreeKind::Empty => false,
        TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::LAZY) => false,
        _ => true,
    }
}

/// `widened` marks a `private` member the companion reads: nsc renames such a
/// member and exposes it, because the JVM would reject the cross-class access.
pub(crate) fn field_access_flags(mods: Flags, widened: bool) -> u16 {
    let mut acc = if mods.contains(Flags::PRIVATE) && !widened {
        ACC_PRIVATE
    } else {
        ACC_PUBLIC
    };
    if !mods.contains(Flags::MUTABLE) {
        acc |= ACC_FINAL;
    }
    if mods.contains(Flags::VOLATILE) {
        acc |= ACC_VOLATILE;
    }
    if mods.contains(Flags::TRANSIENT) {
        acc |= ACC_TRANSIENT;
    }
    acc
}

/// The classfile access of a case class's synthetic `apply` / `copy`.
///
/// `-Xsource-features:case-apply-copy-access` copies the primary
/// constructor's modifier onto them (`crates/typer/src/check.rs`,
/// `CtorAccess`); without the feature the symbol carries no access flag and
/// this is `ACC_PUBLIC`, as before. `private[p]` and `protected` are public in
/// the classfile, matching what `javap -p` shows for scalac 2.13.16:
///
/// ```text
/// // case class C private (x: Int)   -Xsource-features:case-apply-copy-access
/// private xtest.C apply(int);        // in C$
/// private xtest.C copy(int);         // in C
/// // case class E private[xtest] (x: Int) — the qualifier is erased away
/// public xtest.E apply(int);
/// ```
///
/// `access_widened` is the existing escape hatch for a `private` member the
/// typer saw read from another class file (`expand_private.rs`): the JVM
/// would reject `ACC_PRIVATE` there.
pub(crate) fn synthetic_case_member_access(st: &SymbolTable, sym: SymbolId) -> u16 {
    if sym.is_none() {
        return ACC_PUBLIC;
    }
    let s = st.get(sym);
    if s.flags.contains(Flags::PRIVATE) && s.private_within.is_none() && !s.access_widened {
        ACC_PRIVATE
    } else {
        ACC_PUBLIC
    }
}

/// The synthetic `apply` of a case class's companion, if the typer made one.
pub(crate) fn case_apply_sym(st: &SymbolTable, class_id: SymbolId) -> SymbolId {
    let Some(module) = st.companion_module(class_id) else {
        return SymbolId::NONE;
    };
    let module_cls = st.module_class_of(module);
    st.get(module_cls)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).name == "apply" && st.get(m).flags.contains(Flags::SYNTHETIC))
        .unwrap_or(SymbolId::NONE)
}

pub(crate) fn method_access_flags(mods: Flags, widened: bool) -> u16 {
    let mut acc = if mods.contains(Flags::PRIVATE) && !widened {
        ACC_PRIVATE
    } else {
        ACC_PUBLIC
    };
    if mods.contains(Flags::NATIVE) {
        acc |= ACC_NATIVE;
    }
    if mods.contains(Flags::STATIC) {
        acc |= ACC_STATIC;
    }
    acc
}

pub(crate) fn peel_fun(tree: &Tree) -> &Tree {
    match &tree.kind {
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => peel_fun(fun),
        _ => tree,
    }
}

pub(crate) fn is_presuper_val(tree: &Tree) -> bool {
    matches!(
        &tree.kind,
        TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::PRESUPER)
    )
}

pub(crate) fn flatten_apply_owned<'a>(fun: &'a Tree, args: &'a [Tree]) -> (&'a Tree, Vec<Tree>) {
    let mut all = args.to_vec();
    let mut f = fun;
    loop {
        let p = peel_fun(f);
        match &p.kind {
            TreeKind::Apply {
                fun: inner,
                args: ia,
            } if !matches!(&peel_fun(inner).kind, TreeKind::New { .. })
                // Only a curried *method*'s clauses are one JVM call: a
                // partial application leaves a method type behind. An inner
                // application whose *result* is a function value
                // (`f.curried(3)(4)`, `Function.untupled(g)(1, 2)`) is a call
                // of its own, and merging the lists would push the outer
                // arguments onto the inner `apply`.
                && !matches!(p.ty, Type::Function { .. }) =>
            {
                let mut combined = ia.clone();
                combined.append(&mut all);
                all = combined;
                f = inner;
            }
            _ => return (p, all),
        }
    }
}

// ---------------------------------------------------------------------------
// walk
// ---------------------------------------------------------------------------

/// Every subtree of `tree` that can hold a term. Used by passes that look for
/// one specific shape anywhere in a unit — a declaration can sit in a template
/// body, a method body, an `if` branch, a `match` case or a lambda, and a pass
/// that only descends through templates silently misses all but the first.
pub(crate) fn for_each_term_child(tree: &Tree, f: &mut impl FnMut(&Tree)) {
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
            vparamss, impl_, ..
        } => {
            for p in vparamss.iter().flatten() {
                f(p);
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
            vparamss, tpt, rhs, ..
        } => {
            for p in vparamss.iter().flatten() {
                f(p);
            }
            f(tpt);
            f(rhs);
        }
        TreeKind::TypeDef { rhs, .. } => f(rhs),
        TreeKind::LabelDef { params, rhs, .. } => {
            for p in params {
                f(p);
            }
            f(rhs);
        }
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
        TreeKind::TypeApply { fun, args }
        | TreeKind::Apply { fun, args }
        | TreeKind::UnApply { fun, args } => {
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

/// The JVM box for a Scala primitive; `x.toString` on an `Int` dispatches on it.
/// The `iN`/`lN`/`fN`/`dN` sequence that turns a primitive of `code[0]` into
/// one of `code[1]` (JVM descriptor letters). `Byte`, `Short` and `Char` are
/// `int` on the stack, so a conversion *to* one of them is the `int` sequence
/// followed by the truncating `i2b`/`i2s`/`i2c`, and a conversion *from* one
/// starts from `int` with no instruction of its own.
pub(crate) fn emit_num_conv(asm: &mut Assembler, code: &str) {
    let b = code.as_bytes();
    let (from, to) = (b[0], b[1]);
    if from == to {
        return;
    }
    // Step 1: down to the JVM computational type of `to`'s int-width family.
    match (from, to) {
        // int-like source: nothing to do, it is already an `int`.
        (b'B' | b'S' | b'C' | b'I', b'B' | b'S' | b'C' | b'I') => {}
        (b'B' | b'S' | b'C' | b'I', b'J') => asm.i2l(),
        (b'B' | b'S' | b'C' | b'I', b'F') => asm.i2f(),
        (b'B' | b'S' | b'C' | b'I', _) => asm.i2d(),
        (b'J', b'B' | b'S' | b'C' | b'I') => asm.l2i(),
        (b'J', b'F') => asm.l2f(),
        (b'J', _) => asm.l2d(),
        (b'F', b'B' | b'S' | b'C' | b'I') => asm.f2i(),
        (b'F', b'J') => asm.f2l(),
        (b'F', _) => asm.f2d(),
        (_, b'B' | b'S' | b'C' | b'I') => asm.d2i(),
        (_, b'J') => asm.d2l(),
        (_, _) => asm.d2f(),
    }
    // Step 2: truncate the `int` to the narrower target.
    match to {
        b'B' => asm.i2b(),
        b'S' => asm.i2s(),
        b'C' => asm.i2c(),
        _ => {}
    }
}

pub(crate) fn is_boxed_primitive(jvm: &str) -> bool {
    matches!(
        jvm,
        "java/lang/Integer"
            | "java/lang/Long"
            | "java/lang/Double"
            | "java/lang/Float"
            | "java/lang/Short"
            | "java/lang/Byte"
            | "java/lang/Character"
            | "java/lang/Boolean"
    )
}

/// The primitive a `BoxValue` / `UnboxValue` intrinsic names.
pub(crate) fn prim_of_desc(desc: &str) -> Type {
    match desc {
        "Z" => Type::Boolean,
        "B" => Type::Byte,
        "S" => Type::Short,
        "C" => Type::Char,
        "I" => Type::Int,
        "J" => Type::Long,
        "F" => Type::Float,
        _ => Type::Double,
    }
}

/// The conversion instruction between two JVM primitives, for the places that
/// hand a value to something expecting a wider one. `widen_numeric` only knows
/// the arithmetic-promotion cases; boxing needs `Char -> Int` (`val i:
/// java.lang.Integer = 'c'` is legal in nsc) and `Int -> Float` as well.
pub(crate) fn widen_primitive(asm: &mut Assembler, from: &Type, to: &Type) {
    let from = from.widen_constant();
    if from == *to {
        return;
    }
    let int_shaped = matches!(
        from,
        Type::Int | Type::Char | Type::Short | Type::Byte | Type::Boolean
    );
    match to {
        Type::Long if int_shaped => asm.i2l(),
        Type::Long if matches!(from, Type::Long) => {}
        Type::Float if int_shaped => asm.i2f(),
        Type::Float if matches!(from, Type::Long) => asm.l2f(),
        Type::Double if int_shaped => asm.i2d(),
        Type::Double if matches!(from, Type::Long) => asm.l2d(),
        Type::Double if matches!(from, Type::Float) => asm.f2d(),
        // `Int`, `Char`, `Short`, `Byte` and `Boolean` all live in an int-sized
        // slot; a narrowing `i2c`/`i2s`/`i2b` would be a real conversion, but
        // no widening conversion reaches this arm.
        _ => {}
    }
}

/// `1 + 2.5` reaches `Double.+` with an `int` receiver; the JVM needs the
/// widening instruction before the arithmetic op.
pub(crate) fn widen_numeric(asm: &mut Assembler, from: &Type, to: &Type) {
    match (from.widen_constant(), to) {
        (Type::Int, Type::Long) => asm.i2l(),
        (Type::Char, Type::Long) => asm.i2l(),
        (Type::Short, Type::Long) => asm.i2l(),
        (Type::Byte, Type::Long) => asm.i2l(),
        (Type::Int, Type::Double) => asm.i2d(),
        (Type::Char, Type::Double) => asm.i2d(),
        (Type::Short, Type::Double) => asm.i2d(),
        (Type::Byte, Type::Double) => asm.i2d(),
        (Type::Long, Type::Double) => asm.l2d(),
        (Type::Float, Type::Double) => asm.f2d(),
        // `FloatBin` had no widening at all, so `1 + 2.5f` pushed an `int`
        // where the verifier wanted a `float`.
        (Type::Int, Type::Float) => asm.i2f(),
        (Type::Char, Type::Float) => asm.i2f(),
        (Type::Short, Type::Float) => asm.i2f(),
        (Type::Byte, Type::Float) => asm.i2f(),
        (Type::Long, Type::Float) => asm.l2f(),
        _ => {}
    }
}

pub(crate) fn append_str(asm: &mut Assembler, s: &str) {
    asm.ldc_string(s);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
}

/// The `StringBuilder.append` overload for a field's erased type.
pub(crate) fn append_desc(ty: &Type) -> &'static str {
    match ty {
        Type::Int | Type::Short | Type::Byte => "(I)Ljava/lang/StringBuilder;",
        Type::Long => "(J)Ljava/lang/StringBuilder;",
        Type::Double => "(D)Ljava/lang/StringBuilder;",
        Type::Float => "(F)Ljava/lang/StringBuilder;",
        Type::Char => "(C)Ljava/lang/StringBuilder;",
        Type::Boolean => "(Z)Ljava/lang/StringBuilder;",
        Type::String => "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        _ => "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
    }
}

/// `classOf[T]`: `ldc` of the class constant, or the boxed type's `TYPE`
/// field for a primitive, as nsc emits.
/// `x.getClass` with the receiver already on the stack. A primitive (and
/// `Unit`, whose receiver is the `BoxedUnit` singleton) answers with the
/// `TYPE` constant -- `1.getClass` is `int`, `().getClass` is `void` -- so the
/// receiver is dropped again after being evaluated for its effects.
pub(crate) fn emit_get_class(asm: &mut Assembler, ctx: &EmitCtx, recv_ty: &Type) {
    if !is_jvm_primitive(recv_ty) {
        asm.invokevirtual("java/lang/Object", "getClass", "()Ljava/lang/Class;");
        return;
    }
    match recv_ty.widen_constant() {
        // A `Unit` receiver is a one-slot `BoxedUnit` reference, not two.
        Type::Long | Type::Double => asm.pop2(),
        _ => asm.pop(),
    }
    emit_class_constant(asm, ctx, &recv_ty.widen_constant());
}

pub(crate) fn emit_class_constant(asm: &mut Assembler, ctx: &EmitCtx, ty: &Type) {
    let boxed = |asm: &mut Assembler, owner: &str| {
        asm.getstatic(owner, "TYPE", "Ljava/lang/Class;");
    };
    match ty {
        Type::Int => boxed(asm, "java/lang/Integer"),
        Type::Long => boxed(asm, "java/lang/Long"),
        Type::Double => boxed(asm, "java/lang/Double"),
        Type::Float => boxed(asm, "java/lang/Float"),
        Type::Short => boxed(asm, "java/lang/Short"),
        Type::Byte => boxed(asm, "java/lang/Byte"),
        Type::Char => boxed(asm, "java/lang/Character"),
        Type::Boolean => boxed(asm, "java/lang/Boolean"),
        Type::Unit => boxed(asm, "java/lang/Void"),
        Type::Array(_) => {
            let d = jvm_desc(ctx.st, ty);
            asm.ldc_class(&d);
        }
        other => {
            let n = type_jvm_name(ctx.st, other);
            asm.ldc_class(&n);
        }
    }
}

/// Whether the typer widened this member's access for companion use.
/// The JVM sorts of a method descriptor's parameters, given `desc` starting at
/// `(` (anything after the matching `)` is ignored).
/// Byte offsets at which each field descriptor starts inside a *bare*
/// parameter list (no parentheses). Used to split off the last parameter --
/// `rfind('L')` cannot, because a class name may itself contain an `L`
/// (`Lfoo/BarList;`).
pub(crate) fn split_desc_types(inner: &str) -> Vec<usize> {
    let b = inner.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        out.push(i);
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i < b.len() && b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
        }
        i += 1;
    }
    out
}

/// The parameter part of a method descriptor, parentheses included.
pub(crate) fn desc_params(desc: &str) -> &str {
    match desc.find(')') {
        Some(i) => &desc[..=i],
        None => desc,
    }
}

/// The [`JvmSort`] of a return descriptor (`V`, `I`, `Ljava/lang/String;`, …).
/// Sort of a return type written on its own, without the `(params)` prefix
/// that `desc_ret_sort` expects.
pub(crate) fn ret_str_sort(ret: &str) -> JvmSort {
    match ret.as_bytes().first() {
        Some(b'V') => JvmSort::Void,
        Some(b'J') => JvmSort::Long,
        Some(b'D') => JvmSort::Double,
        Some(b'F') => JvmSort::Float,
        Some(b'Z' | b'B' | b'S' | b'C' | b'I') => JvmSort::Int,
        _ => JvmSort::Ref,
    }
}

/// The parameter descriptors of a method descriptor, one string each.
pub(crate) fn desc_param_strs(desc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = desc.as_bytes();
    let mut i = if b.first() == Some(&b'(') { 1 } else { 0 };
    while i < b.len() && b[i] != b')' {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        if b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
        }
        i += 1;
        out.push(desc[start..i.min(desc.len())].to_string());
    }
    out
}

pub(crate) fn desc_param_sorts(desc: &str) -> Vec<JvmSort> {
    let mut out = Vec::new();
    let b = desc.as_bytes();
    let mut i = if b.first() == Some(&b'(') { 1 } else { 0 };
    while i < b.len() && b[i] != b')' {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        let c = b[i];
        if c == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
        }
        i += 1;
        let is_array = b[start] == b'[';
        out.push(match c {
            _ if is_array => JvmSort::Ref,
            b'J' => JvmSort::Long,
            b'D' => JvmSort::Double,
            b'F' => JvmSort::Float,
            b'Z' | b'B' | b'S' | b'C' | b'I' => JvmSort::Int,
            _ => JvmSort::Ref,
        });
    }
    out
}

pub(crate) fn widened(st: &SymbolTable, sym: SymbolId) -> bool {
    !sym.is_none() && st.get(sym).access_widened
}

/// A trait method that stays genuinely `private` on the JVM (not widened by
/// the typer). Real scalac keeps such a method's implementation entirely
/// inside the interface as a `private` method with a body -- JVMS 4.6
/// forbids `ACC_PRIVATE | ACC_ABSTRACT` on any method, interface ones
/// included. We keep the body on the interface too, but as a
/// `private static <name>$` taking the receiver rather than a `private`
/// instance method — the invariant is the same one: no declaration on the
/// interface at all (nothing outside the trait's own code may call it), and
/// no mixin forwarder on any implementing class.
pub(crate) fn is_trait_private_def(st: &SymbolTable, def: &Tree) -> bool {
    match &def.kind {
        TreeKind::DefDef { mods, .. } => {
            mods.flags.contains(Flags::PRIVATE) && !widened(st, def.sym)
        }
        _ => false,
    }
}

/// The module class of a package's package object, if it has one.
pub(crate) fn package_object_module(st: &SymbolTable, pkg: SymbolId) -> Option<SymbolId> {
    let m = st
        .get(pkg)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).name == "package" && is_module_like(st, m))?;
    Some(module_class_id(st, m))
}

pub(crate) fn is_module_like(st: &SymbolTable, id: SymbolId) -> bool {
    matches!(
        st.get(id).kind,
        scala_rs_typer::SymKind::Module | scala_rs_typer::SymKind::ModuleClass
    )
}

/// `x.##`: nsc calls `Statics.doubleHash` and friends for the numeric types
/// so that `1.0.##` and `1.##` agree, and `anyHash` for everything else.
pub(crate) fn emit_any_hash(asm: &mut Assembler, recv: &Type) {
    let ty = recv.widen_constant();
    let (name, desc) = match ty {
        Type::Double => ("doubleHash", "(D)I"),
        Type::Float => ("floatHash", "(F)I"),
        Type::Long => ("longHash", "(J)I"),
        _ => {
            // A `Unit` receiver is already the `BoxedUnit` singleton by the
            // time it gets here (`gen_select_receiver` / `gen_receiver`
            // materialise it); boxing again would push a second one.
            if is_jvm_primitive(&ty) && !is_unit_like(&ty) {
                emit_box(asm, &ty);
            }
            ("anyHash", "(Ljava/lang/Object;)I")
        }
    };
    asm.invokestatic("scala/runtime/Statics", name, desc);
}

/// Build the `Object[]` a `String.format` call takes, boxing as it goes.
pub(crate) fn emit_format_args(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
) {
    asm.iconst(args.len() as i32);
    asm.anewarray("java/lang/Object");
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) || matches!(a.ty, Type::Unit | Type::NoType) {
            emit_box(asm, &a.ty);
        }
        asm.aastore();
    }
}

/// Does `cls` declare a no-argument method of this name -- the accessor a
/// case class's constructor field gets?
pub(crate) fn has_nullary_accessor(st: &SymbolTable, cls: SymbolId, name: &str) -> bool {
    if cls.is_none() {
        return false;
    }
    st.lookup_member(cls, name).into_iter().any(|m| {
        st.get(m).kind == SymKind::Method
            && matches!(&st.get(m).ty, Type::Method { paramss, .. }
                if paramss.iter().flatten().next().is_none())
    })
}
