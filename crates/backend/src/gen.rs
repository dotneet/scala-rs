//! Walk a typed compilation unit and emit JVM classfiles (major 52).

use crate::classfile::{
    encode_method_name, ClassEmit, EmittedClass, Field, Method, Pool, ACC_ABSTRACT, ACC_BRIDGE,
    ACC_FINAL, ACC_INTERFACE, ACC_NATIVE, ACC_PRIVATE, ACC_PUBLIC, ACC_STATIC, ACC_SUPER,
    ACC_SYNTHETIC, ACC_TRANSIENT, ACC_VOLATILE,
};
use crate::code::Assembler;
use scala_rs_parser::{Flags, Lit, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{Intrinsic, SymKind, SymbolTable};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

/// Options for [`emit_opts`].
#[derive(Clone, Debug, Default)]
pub struct EmitOpts {
    /// Emit invokes against scala-library 2.13 (Option/List/`::`/FunctionN/…).
    ///
    /// The private runtime classfiles are not part of this function; the driver
    /// skips them when this flag is set. Call sites that would miss on the jar
    /// (`List.withFilter`, `List.tail()List`, ArrowAssoc) are rewritten here.
    pub library_abi: bool,
    /// Pre-erasure ScalaSignature pickles, keyed by class symbol id.
    pub pickles: HashMap<u32, Vec<u8>>,
}

/// Walk a typed compilation unit and emit classes (private-runtime ABI).
pub fn emit(tree: &Tree, st: &SymbolTable, source_name: &str) -> Vec<EmittedClass> {
    emit_opts(tree, st, source_name, EmitOpts::default())
}

/// Walk a typed compilation unit and emit classes.
pub fn emit_opts(
    tree: &Tree,
    st: &SymbolTable,
    source_name: &str,
    opts: EmitOpts,
) -> Vec<EmittedClass> {
    let mut g = Gen {
        st,
        source_name,
        out: Vec::new(),
        extras: RefCell::new(Vec::new()),
        lambda_n: Cell::new(0),
        trait_impls: HashMap::new(),
        trait_vals: HashMap::new(),
        library_abi: opts.library_abi,
        pickles: opts.pickles,
        // `scala.runtime.*Ref` exists in both ABIs: on the jar, and as a
        // private-runtime classfile (see `runtime::REF_BOXES`).
        boxed_vars: collect_boxed_vars(tree, st),
    };
    g.collect_trait_impls(tree);
    g.walk(tree);
    g.emit_anon_classes(tree);
    g.out.append(&mut g.extras.borrow_mut());
    g.out
}

struct Gen<'a> {
    st: &'a SymbolTable,
    source_name: &'a str,
    out: Vec<EmittedClass>,
    extras: RefCell<Vec<EmittedClass>>,
    lambda_n: Cell<u32>,
    /// Concrete trait methods, for `$class` static impls and mixin forwarders.
    trait_impls: HashMap<SymbolId, Vec<Tree>>,
    /// Trait `val` definitions with a right-hand side (`T$class.$init$`).
    trait_vals: HashMap<SymbolId, Vec<Tree>>,
    library_abi: bool,
    pickles: HashMap<u32, Vec<u8>>,
    /// Locals boxed into `scala.runtime.IntRef` / `ObjectRef` (library ABI).
    boxed_vars: HashSet<SymbolId>,
}

struct EmitCtx<'a> {
    st: &'a SymbolTable,
    class_sym: SymbolId,
    class_name: &'a str,
    ret_ty: Type,
    extras: &'a RefCell<Vec<EmittedClass>>,
    lambda_n: &'a Cell<u32>,
    source: &'a str,
    /// If generating inside a lambda, field on the lambda class holding the outer `this`.
    outer: Option<(&'a str, &'a str, &'a str)>, // (lambda_class, field, outer_desc)
    library_abi: bool,
    /// Named JVM method being emitted; `NONE` inside lambdas.
    method_sym: SymbolId,
    /// Captured `var`s lowered to `scala.runtime.*Ref`.
    boxed_vars: &'a HashSet<SymbolId>,
}

fn emit_ctx<'a>(
    st: &'a SymbolTable,
    class_sym: SymbolId,
    class_name: &'a str,
    ret_ty: Type,
    extras: &'a RefCell<Vec<EmittedClass>>,
    lambda_n: &'a Cell<u32>,
    source: &'a str,
    library_abi: bool,
    boxed_vars: &'a HashSet<SymbolId>,
) -> EmitCtx<'a> {
    EmitCtx {
        st,
        class_sym,
        class_name,
        ret_ty,
        extras,
        lambda_n,
        source,
        outer: None,
        library_abi,
        method_sym: SymbolId::NONE,
        boxed_vars,
    }
}

fn runtime_ref_class(ty: &Type) -> &'static str {
    match ty.widen_constant() {
        Type::Int => "scala/runtime/IntRef",
        Type::Long => "scala/runtime/LongRef",
        Type::Double => "scala/runtime/DoubleRef",
        Type::Float => "scala/runtime/FloatRef",
        Type::Boolean => "scala/runtime/BooleanRef",
        Type::Byte => "scala/runtime/ByteRef",
        Type::Short => "scala/runtime/ShortRef",
        Type::Char => "scala/runtime/CharRef",
        _ => "scala/runtime/ObjectRef",
    }
}

fn runtime_ref_elem_desc(ty: &Type) -> &'static str {
    match ty.widen_constant() {
        Type::Int => "I",
        Type::Long => "J",
        Type::Double => "D",
        Type::Float => "F",
        Type::Boolean => "Z",
        Type::Byte => "B",
        Type::Short => "S",
        Type::Char => "C",
        _ => "Ljava/lang/Object;",
    }
}

fn runtime_ref_create_desc(ty: &Type) -> String {
    format!("({})L{};", runtime_ref_elem_desc(ty), runtime_ref_class(ty))
}

fn is_boxed_var(ctx: &EmitCtx, id: SymbolId) -> bool {
    !id.is_none() && ctx.boxed_vars.contains(&id)
}

fn emit_runtime_ref_create(asm: &mut Assembler, ty: &Type) {
    let cls = runtime_ref_class(ty);
    asm.invokestatic(cls, "create", &runtime_ref_create_desc(ty));
}

fn load_runtime_ref_elem(asm: &mut Assembler, ctx: &EmitCtx, ty: &Type) {
    let cls = runtime_ref_class(ty);
    let elem = runtime_ref_elem_desc(ty);
    asm.getfield(cls, "elem", elem);
    if elem == "Ljava/lang/Object;" {
        if matches!(ty, Type::String) {
            asm.checkcast("java/lang/String");
        } else if let Some(cn) = checkcast_internal(ctx.st, ty) {
            if cn != "java/lang/Object" {
                asm.checkcast(&cn);
            }
        }
    }
}

fn store_runtime_ref_elem(asm: &mut Assembler, ty: &Type) {
    let cls = runtime_ref_class(ty);
    let elem = runtime_ref_elem_desc(ty);
    if elem == "Ljava/lang/Object;" && is_jvm_primitive(ty) && !is_unit_like(ty) {
        emit_box(asm, ty);
    }
    asm.putfield(cls, "elem", elem);
}

fn jvm_desc_maybe_boxed(
    st: &SymbolTable,
    ty: &Type,
    id: SymbolId,
    boxed: &HashSet<SymbolId>,
) -> String {
    if !id.is_none() && boxed.contains(&id) {
        format!("L{};", runtime_ref_class(ty))
    } else {
        jvm_desc(st, ty)
    }
}

// ---------------------------------------------------------------------------
// captured enclosing-method locals (`new T { … }`, local `class`)
// ---------------------------------------------------------------------------

/// Enclosing-method locals a class defined inside a method has to receive.
/// Filled by the typer's `anon_capture` pass; empty for every other class.
fn class_captures(st: &SymbolTable, class_id: SymbolId) -> &[SymbolId] {
    if class_id.is_none() {
        return &[];
    }
    &st.get(class_id).captures
}

/// Field / constructor-parameter name of the `idx`-th capture (nsc: `x$1`).
fn capture_field_name(st: &SymbolTable, id: SymbolId, idx: usize) -> String {
    format!("{}${}", st.get(id).name, idx + 1)
}

/// Descriptor of a captured value; a `scala.runtime.*Ref` for captured `var`s.
fn capture_field_desc(st: &SymbolTable, boxed: &HashSet<SymbolId>, id: SymbolId) -> String {
    jvm_desc_maybe_boxed(st, &st.get(id).ty, id, boxed)
}

fn capture_field_sort(boxed: &HashSet<SymbolId>, st: &SymbolTable, id: SymbolId) -> JvmSort {
    if boxed.contains(&id) {
        JvmSort::Ref
    } else {
        jvm_sort(&st.get(id).ty)
    }
}

/// The capture constructor parameters of `class_id`, as descriptor text.
fn capture_params_desc(st: &SymbolTable, boxed: &HashSet<SymbolId>, class_id: SymbolId) -> String {
    class_captures(st, class_id)
        .iter()
        .map(|c| capture_field_desc(st, boxed, *c))
        .collect()
}

/// Splice extra parameter descriptors in front of the `)` of `desc`.
fn desc_with_extra_params(desc: &str, extra: &str) -> String {
    if extra.is_empty() {
        return desc.to_string();
    }
    match desc.rfind(')') {
        Some(i) => format!("{}{}{}", &desc[..i], extra, &desc[i..]),
        None => desc.to_string(),
    }
}

/// `(symbol, field name, field descriptor, JVM sort)` per capture.
type CaptureSlots = Vec<(SymbolId, String, String, JvmSort)>;

fn capture_slots(st: &SymbolTable, boxed: &HashSet<SymbolId>, class_id: SymbolId) -> CaptureSlots {
    class_captures(st, class_id)
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                *c,
                capture_field_name(st, *c, i),
                capture_field_desc(st, boxed, *c),
                capture_field_sort(boxed, st, *c),
            )
        })
        .collect()
}

/// Read the capture fields into fresh locals at method entry, so the ordinary
/// `Ident` path keeps finding the enclosing-method symbols in the frame.
fn emit_capture_prologue(
    asm: &mut Assembler,
    frame: &mut Frame,
    class_name: &str,
    caps: &CaptureSlots,
) {
    for (id, fname, fdesc, sort) in caps {
        asm.aload(0);
        asm.getfield(class_name, fname, fdesc);
        let slot = frame.alloc(*id, *sort);
        store(asm, slot, *sort);
    }
}

/// Push the current value of a captured local for a `new` of a capturing class.
fn load_capture_arg(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, id: SymbolId) {
    if let Some((slot, sort)) = frame.get(id) {
        if is_boxed_var(ctx, id) {
            // Forward the IntRef/ObjectRef itself, not its `elem`.
            load(asm, slot, JvmSort::Ref);
        } else {
            load(asm, slot, sort);
        }
        return;
    }
    // Not a local here: we are inside a class that captured it as well.
    let own = class_captures(ctx.st, ctx.class_sym);
    if let Some(i) = own.iter().position(|c| *c == id) {
        load_this(asm, ctx);
        asm.getfield(
            &class_internal(ctx.st, ctx.class_sym),
            &capture_field_name(ctx.st, id, i),
            &capture_field_desc(ctx.st, ctx.boxed_vars, id),
        );
        return;
    }
    throw_runtime(asm, &format!("cannot capture {}", ctx.st.get(id).name));
    asm.aconst_null();
}

fn def_is_synthetic(st: &SymbolTable, def: &Tree) -> bool {
    if !def.sym.is_none() && st.get(def.sym).flags.contains(Flags::SYNTHETIC) {
        return true;
    }
    if let TreeKind::DefDef { mods, .. } = &def.kind {
        return mods.flags.contains(Flags::SYNTHETIC);
    }
    false
}

fn def_method_desc_boxed(st: &SymbolTable, def: &Tree, boxed: &HashSet<SymbolId>) -> String {
    let synthetic = def_is_synthetic(st, def);
    let mut s = String::from("(");
    if let TreeKind::DefDef { vparamss, .. } = &def.kind {
        for p in vparamss.iter().flatten() {
            let ty = if !p.ty.is_no_type() && !p.ty.is_error() {
                p.ty.clone()
            } else if !p.sym.is_none() {
                st.get(p.sym).ty.clone()
            } else {
                Type::Any
            };
            if synthetic {
                s.push_str(&jvm_desc_maybe_boxed(st, &ty, p.sym, boxed));
            } else {
                s.push_str(&jvm_desc(st, &ty));
            }
        }
    }
    s.push(')');
    s.push_str(&jvm_desc(st, &method_ret_ty(def)));
    s
}

fn method_desc_boxed(st: &SymbolTable, id: SymbolId, boxed: &HashSet<SymbolId>) -> String {
    let s = st.get(id);
    if s.name == "<init>" || s.jvm_name.starts_with('(') {
        return method_desc_from_sym(st, id);
    }
    let synthetic = s.flags.contains(Flags::SYNTHETIC);
    let params: Vec<Type> = method_params_from_sym(st, id);
    let ret = method_ret_from_sym(st, id);
    let mut d = String::from("(");
    for (i, p) in params.iter().enumerate() {
        let pid = s.params.get(i).copied().unwrap_or(SymbolId::NONE);
        if synthetic {
            d.push_str(&jvm_desc_maybe_boxed(st, p, pid, boxed));
        } else {
            d.push_str(&jvm_desc(st, p));
        }
    }
    d.push(')');
    d.push_str(&jvm_desc(st, &ret));
    d
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JvmSort {
    Int,
    Long,
    Float,
    Double,
    Ref,
    Void,
}

impl JvmSort {
    fn slots(self) -> u16 {
        match self {
            JvmSort::Long | JvmSort::Double => 2,
            JvmSort::Void => 0,
            _ => 1,
        }
    }
}

struct Frame {
    locals: HashMap<SymbolId, (u16, JvmSort)>,
    next_slot: u16,
}

impl Frame {
    fn instance() -> Self {
        Frame {
            locals: HashMap::new(),
            next_slot: 1,
        }
    }

    fn alloc(&mut self, id: SymbolId, sort: JvmSort) -> u16 {
        let slot = self.next_slot;
        if !id.is_none() {
            self.locals.insert(id, (slot, sort));
        }
        self.next_slot += sort.slots();
        slot
    }

    fn alloc_tmp(&mut self, sort: JvmSort) -> u16 {
        let slot = self.next_slot;
        self.next_slot += sort.slots();
        slot
    }

    fn get(&self, id: SymbolId) -> Option<(u16, JvmSort)> {
        self.locals.get(&id).copied()
    }
}

struct ClassBuilder {
    access: u16,
    this_name: String,
    super_name: String,
    interfaces: Vec<String>,
    fields: Vec<Field>,
    methods: Vec<Method>,
    pool: Pool,
    source: String,
    scala_signature: Option<String>,
    scala_raw: bool,
}

impl ClassBuilder {
    fn new(this_name: String, source: &str) -> Self {
        ClassBuilder {
            access: ACC_PUBLIC | ACC_SUPER,
            this_name,
            super_name: "java/lang/Object".into(),
            interfaces: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            pool: Pool::new(),
            source: source.to_string(),
            scala_signature: None,
            scala_raw: false,
        }
    }

    fn add_code(
        &mut self,
        access: u16,
        name: &str,
        desc: &str,
        max_locals: u16,
        gen: impl FnOnce(&mut Assembler),
    ) {
        let mut asm = Assembler::with_pool(std::mem::take(&mut self.pool), max_locals.max(1));
        asm.init_method(access, name, desc, &self.this_name);
        gen(&mut asm);
        let (code, pool) = asm.finish();
        self.pool = pool;
        self.methods.push(Method {
            access,
            name: encode_method_name(name),
            desc: desc.to_string(),
            code: Some(code),
            java_annots: Vec::new(),
        });
    }

    fn add_abstract(&mut self, access: u16, name: &str, desc: &str) {
        self.methods.push(Method {
            access,
            name: encode_method_name(name),
            desc: desc.to_string(),
            code: None,
            java_annots: Vec::new(),
        });
    }

    fn add_java_annot_to_last(&mut self, desc: &str) {
        if let Some(m) = self.methods.last_mut() {
            if !m.java_annots.iter().any(|a| a == desc) {
                m.java_annots.push(desc.to_string());
            }
        }
    }

    fn finish(self) -> EmittedClass {
        let this_name = self.this_name.clone();
        let class = ClassEmit {
            access: self.access,
            this_name: self.this_name,
            super_name: self.super_name,
            interfaces: self.interfaces,
            fields: self.fields,
            methods: self.methods,
            source: self.source,
            scala_signature: self.scala_signature,
            scala_raw: self.scala_raw,
        };
        let bytes = class.write_with_pool(self.pool).expect("classfile write");
        EmittedClass {
            internal_name: this_name,
            bytes,
        }
    }
}

fn attach_scala_sig(
    b: &mut ClassBuilder,
    st: &SymbolTable,
    class_id: SymbolId,
    pickles: &HashMap<u32, Vec<u8>>,
) {
    if class_id.is_none() {
        return;
    }
    let raw = pickles
        .get(&class_id.0)
        .cloned()
        .unwrap_or_else(|| crate::pickle::pickle_class(st, class_id));
    if raw.is_empty() {
        return;
    }
    b.scala_signature = Some(crate::pickle::encode_to_annotation_string(&raw));
}

// ---------------------------------------------------------------------------
// descriptors
// ---------------------------------------------------------------------------

fn jvm_sort(ty: &Type) -> JvmSort {
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

fn is_unit_like(ty: &Type) -> bool {
    matches!(ty, Type::Unit | Type::NoType)
}

fn class_internal(st: &SymbolTable, id: SymbolId) -> String {
    st.jvm_internal(id)
}

fn jvm_desc(st: &SymbolTable, ty: &Type) -> String {
    match ty {
        Type::Unit | Type::NoType | Type::Nothing => "V".into(),
        Type::Boolean => "Z".into(),
        Type::Byte => "B".into(),
        Type::Short => "S".into(),
        Type::Int => "I".into(),
        Type::Long => "J".into(),
        Type::Float => "F".into(),
        Type::Double => "D".into(),
        Type::Char => "C".into(),
        Type::String => "Ljava/lang/String;".into(),
        Type::Array(t) => format!("[{}", jvm_desc(st, t)),
        Type::Class { sym, .. } => format!("L{};", class_internal(st, *sym)),
        Type::ModuleRef(sym) => format!("L{};", class_internal(st, *sym)),
        Type::Any | Type::AnyRef | Type::AnyVal | Type::Null | Type::Error => {
            "Ljava/lang/Object;".into()
        }
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
            format!("[{}", jvm_desc(st, &args[0]))
        }
        Type::Named { name, .. } => {
            let n = name.replace('.', "/");
            format!("L{n};")
        }
        Type::Overload(_) => "Ljava/lang/Object;".into(),
    }
}

fn jvm_method_desc(st: &SymbolTable, params: &[Type], ret: &Type) -> String {
    let mut s = String::from("(");
    for p in params {
        s.push_str(&jvm_desc(st, p));
    }
    s.push(')');
    s.push_str(&jvm_desc(st, ret));
    s
}

fn method_ret_ty(def: &Tree) -> Type {
    match &def.ty {
        Type::Method { ret, .. } => (**ret).clone(),
        Type::Function { ret, .. } => (**ret).clone(),
        t if !t.is_no_type() => t.clone(),
        _ => Type::Unit,
    }
}

fn def_param_types(st: &SymbolTable, def: &Tree) -> Vec<Type> {
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

fn def_method_desc(st: &SymbolTable, def: &Tree) -> String {
    jvm_method_desc(st, &def_param_types(st, def), &method_ret_ty(def))
}

fn method_params_from_sym(st: &SymbolTable, id: SymbolId) -> Vec<Type> {
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

fn method_ret_from_sym(st: &SymbolTable, id: SymbolId) -> Type {
    match &st.get(id).ty {
        Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
        t => t.clone(),
    }
}

/// How an erasure bridge has to convert a value of type `from` to type `to`.
enum Adapt {
    None,
    Cast(String),
    Box(Type),
    Unbox(Type),
}

fn param_adapt(st: &SymbolTable, from: &Type, to: &Type) -> Adapt {
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

fn emit_adapt(asm: &mut Assembler, adapt: &Adapt) {
    match adapt {
        Adapt::None => {}
        Adapt::Cast(cn) => asm.checkcast(cn),
        Adapt::Box(ty) => emit_box(asm, ty),
        Adapt::Unbox(ty) => emit_unbox(asm, ty),
    }
}

fn checkcast_internal(st: &SymbolTable, ty: &Type) -> Option<String> {
    match ty {
        Type::Class { sym, .. } | Type::ModuleRef(sym) => Some(class_internal(st, *sym)),
        Type::String => Some("java/lang/String".into()),
        Type::Function { params, .. } => Some(format!("scala/Function{}", params.len())),
        Type::Named { name, .. } => Some(name.replace('.', "/")),
        _ => None,
    }
}

fn method_desc_from_sym(st: &SymbolTable, id: SymbolId) -> String {
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
fn with_enclosing_outer_param(st: &SymbolTable, class_id: SymbolId, desc: &str) -> String {
    let Some(outer) = enclosing_instance(st, class_id) else {
        return desc.to_string();
    };
    let outer_ty = format!("L{};", class_internal(st, outer));
    let Some(rest) = desc.strip_prefix('(') else {
        return desc.to_string();
    };
    if rest.starts_with(&outer_ty) {
        return desc.to_string();
    }
    format!("({outer_ty}{rest}")
}

fn ctor_desc(st: &SymbolTable, class_id: SymbolId, args: &[Tree]) -> String {
    if let Some(d) = java_ctor_desc(st, class_id, args.len()) {
        return d;
    }
    if let Some(id) = pick_init_sym(st, class_id, args) {
        return with_enclosing_outer_param(st, class_id, &method_desc_from_sym(st, id));
    }
    let mut d = String::from("(");
    if let Some(outer) = enclosing_instance(st, class_id) {
        d.push_str(&format!("L{};", class_internal(st, outer)));
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

fn pick_init_sym(st: &SymbolTable, class_id: SymbolId, args: &[Tree]) -> Option<SymbolId> {
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

fn parent_super_ctor(
    st: &SymbolTable,
    parents: &[Tree],
    super_name: &str,
) -> (String, String, Vec<Tree>) {
    for p in parents {
        if let TreeKind::Apply { args, .. } = &p.kind {
            if !p.sym.is_none() && st.get(p.sym).name == "<init>" {
                let owner = class_internal(st, st.get(p.sym).owner);
                return (owner, method_desc_from_sym(st, p.sym), args.clone());
            }
            if let Some(cls) = st.class_sym_of(&p.ty) {
                let owner = class_internal(st, cls);
                if owner == super_name || super_name == "java/lang/Object" {
                    let desc = ctor_desc(st, cls, args);
                    return (owner, desc, args.clone());
                }
            }
        }
        if !p.sym.is_none() && st.get(p.sym).name == "<init>" {
            let owner = class_internal(st, st.get(p.sym).owner);
            return (owner, method_desc_from_sym(st, p.sym), Vec::new());
        }
    }
    (super_name.to_string(), "()V".into(), Vec::new())
}

/// Java `<init>` descriptors come from the classfile (`(Ljava/lang/Object;…)V`),
/// not from the Scala argument types (`String` would emit the wrong desc).
fn java_ctor_desc(st: &SymbolTable, class_id: SymbolId, nargs: usize) -> Option<String> {
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

fn enclosing_instance(st: &SymbolTable, class_id: SymbolId) -> Option<SymbolId> {
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
    let mut in_method = false;
    while !owner.is_none() && matches!(st.get(owner).kind, SymKind::Method | SymKind::Term) {
        in_method = true;
        owner = st.get(owner).owner;
    }
    if owner.is_none() {
        return None;
    }
    let o = st.get(owner);
    if o.kind != SymKind::Class || o.flags.contains(Flags::MODULE) {
        return None;
    }
    // A class *member* of a trait has no enclosing instance (the trait is an
    // interface), but a class written inside a trait *method* still needs the
    // receiver to reach the trait's members.
    if (o.flags.contains(Flags::TRAIT) || o.flags.contains(Flags::INTERFACE)) && !in_method {
        return None;
    }
    Some(owner)
}

fn linearize(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    fn skip_parent(st: &SymbolTable, p: SymbolId) -> bool {
        let n = st.get(p).name.as_str();
        n == "Any" || n == "AnyRef" || n == "AnyVal" || n == "Object"
    }
    fn parents_of(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
        st.get(cls)
            .parents
            .iter()
            .filter_map(|p| st.class_sym_of(p))
            .filter(|p| !skip_parent(st, *p))
            .collect()
    }
    fn lin(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
        let parents = parents_of(st, cls);
        let mut lists: Vec<Vec<SymbolId>> = parents.iter().rev().map(|p| lin(st, *p)).collect();
        lists.push(parents.iter().rev().copied().collect());
        let mut out = vec![cls];
        out.extend(c3_merge(lists));
        out
    }
    fn c3_merge(mut lists: Vec<Vec<SymbolId>>) -> Vec<SymbolId> {
        let mut out = Vec::new();
        loop {
            lists.retain(|l| !l.is_empty());
            if lists.is_empty() {
                break;
            }
            let mut chosen = None;
            for l in &lists {
                let h = l[0];
                let in_tail = lists.iter().any(|o| o.iter().skip(1).any(|&x| x == h));
                if !in_tail {
                    chosen = Some(h);
                    break;
                }
            }
            let h = match chosen {
                Some(h) => h,
                None => lists[0][0],
            };
            out.push(h);
            for l in &mut lists {
                if l.first() == Some(&h) {
                    l.remove(0);
                }
            }
        }
        out
    }
    lin(st, cls)
}

fn super_accessor_name(st: &SymbolTable, trait_id: SymbolId, method: &str) -> String {
    format!("{}$$super${}", st.get(trait_id).name, method)
}

fn setter_name(field: &str) -> String {
    format!("$init$set${field}")
}

fn tree_contains_super(tree: &Tree) -> bool {
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

fn needs_super_accessor(def: &Tree) -> bool {
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

fn is_star_pat(pat: &Tree) -> bool {
    match &pat.kind {
        TreeKind::Star { .. } => true,
        TreeKind::Bind { body, .. } => is_star_pat(body),
        TreeKind::Typed { expr, .. } => is_star_pat(expr),
        _ => false,
    }
}

fn val_tree_ty(st: &SymbolTable, vd: &Tree) -> Type {
    if !vd.ty.is_no_type() {
        vd.ty.clone()
    } else if !vd.sym.is_none() {
        st.get(vd.sym).ty.clone()
    } else {
        Type::Any
    }
}

fn is_trait_owned_term(st: &SymbolTable, id: SymbolId) -> bool {
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

fn trait_static_desc(iface: &str, inst_desc: &str) -> String {
    let rest = inst_desc.strip_prefix('(').unwrap_or(inst_desc);
    format!("(L{iface};{rest}")
}

fn type_jvm_name(st: &SymbolTable, ty: &Type) -> String {
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

fn is_interface_sym(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::INTERFACE)
}

/// True when `owner` is `current` or a parent in the extends/with graph.
/// Self types are *not* walked: a trait `self: Foo =>` must checkcast `$this`.
fn is_owner_compatible(st: &SymbolTable, current: SymbolId, owner: SymbolId) -> bool {
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

/// Push the instance that owns `owner`'s members: `this`, or the `$outer`
/// chain of the class being emitted when the member lives further out.
fn load_owner_instance(asm: &mut Assembler, ctx: &EmitCtx, owner: SymbolId) {
    load_this(asm, ctx);
    let mut cur = ctx.class_sym;
    while !cur.is_none() && !owner.is_none() && !is_owner_compatible(ctx.st, cur, owner) {
        let Some(o) = enclosing_instance(ctx.st, cur) else {
            break;
        };
        asm.getfield(
            &class_internal(ctx.st, cur),
            "$outer",
            &format!("L{};", class_internal(ctx.st, o)),
        );
        cur = o;
    }
    if !is_owner_compatible(ctx.st, cur, owner) {
        let kind = ctx.st.get(owner).kind;
        if matches!(kind, SymKind::Class | SymKind::ModuleClass) || is_interface_sym(ctx.st, owner)
        {
            asm.checkcast(&class_internal(ctx.st, owner));
        }
    }
}

fn maybe_checkcast_owner(asm: &mut Assembler, ctx: &EmitCtx, owner: SymbolId) {
    if is_owner_compatible(ctx.st, ctx.class_sym, owner) {
        return;
    }
    let kind = ctx.st.get(owner).kind;
    if matches!(kind, SymKind::Class | SymKind::ModuleClass) || is_interface_sym(ctx.st, owner) {
        let jn = class_internal(ctx.st, owner);
        asm.checkcast(&jn);
    }
}

fn checkcast_refined_receiver(
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
fn checkcast_erased_method_receiver(asm: &mut Assembler, ctx: &EmitCtx, fun: &Tree) {
    if fun.sym.is_none() {
        return;
    }
    let s = ctx.st.get(fun.sym);
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
    if fun_is_super(fun) {
        return;
    }
    let jn = class_internal(ctx.st, s.owner);
    if jn.is_empty() || jn == "java/lang/Object" || jn.starts_with('[') {
        return;
    }
    asm.checkcast(&jn);
}

fn is_module_class(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    s.kind == SymKind::ModuleClass || s.kind == SymKind::Module || s.flags.contains(Flags::MODULE)
}

fn module_class_id(st: &SymbolTable, id: SymbolId) -> SymbolId {
    match st.get(id).ty {
        Type::ModuleRef(c) => c,
        _ => id,
    }
}

fn strip_module_dollar(name: &str) -> String {
    if let Some(rest) = name.strip_suffix('$') {
        rest.to_string()
    } else {
        name.to_string()
    }
}

fn split_parents(st: &SymbolTable, parents: &[Tree]) -> (String, Vec<String>) {
    let mut super_name = "java/lang/Object".to_string();
    let mut ifaces = Vec::new();
    let mut found_class = false;
    for p in parents {
        let id = st
            .class_sym_of(&p.ty)
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
    (super_name, ifaces)
}

fn class_extends_named(st: &SymbolTable, id: SymbolId, name: &str) -> bool {
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

fn extends_delayed_init(st: &SymbolTable, id: SymbolId) -> bool {
    class_extends_named(st, id, "DelayedInit") || class_extends_named(st, id, "App")
}

fn extends_app(st: &SymbolTable, id: SymbolId) -> bool {
    class_extends_named(st, id, "App")
}

fn is_delayed_ctor_stat(t: &Tree) -> bool {
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

fn field_access_flags(mods: Flags) -> u16 {
    let mut acc = if mods.contains(Flags::PRIVATE) {
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

fn method_access_flags(mods: Flags) -> u16 {
    let mut acc = if mods.contains(Flags::PRIVATE) {
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

fn peel_fun(tree: &Tree) -> &Tree {
    match &tree.kind {
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => peel_fun(fun),
        _ => tree,
    }
}

fn is_presuper_val(tree: &Tree) -> bool {
    matches!(
        &tree.kind,
        TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::PRESUPER)
    )
}

fn flatten_apply_owned<'a>(fun: &'a Tree, args: &'a [Tree]) -> (&'a Tree, Vec<Tree>) {
    let mut all = args.to_vec();
    let mut f = fun;
    loop {
        let p = peel_fun(f);
        match &p.kind {
            TreeKind::Apply {
                fun: inner,
                args: ia,
            } if !matches!(&peel_fun(inner).kind, TreeKind::New { .. }) => {
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

impl<'a> Gen<'a> {
    fn collect_trait_impls(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.collect_trait_impls(s);
                }
            }
            TreeKind::ClassDef { mods, impl_, .. } => {
                if mods.flags.contains(Flags::TRAIT) {
                    let methods: Vec<Tree> = impl_
                        .body
                        .iter()
                        .filter(|s| match &s.kind {
                            TreeKind::DefDef { rhs, name, .. } => {
                                !rhs.is_empty() && name != "<init>" && name != "<clinit>"
                            }
                            _ => false,
                        })
                        .cloned()
                        .collect();
                    if !methods.is_empty() && !tree.sym.is_none() {
                        self.trait_impls.insert(tree.sym, methods);
                    }
                    let vals: Vec<Tree> = impl_
                        .body
                        .iter()
                        .filter(|s| match &s.kind {
                            TreeKind::ValDef { rhs, mods, .. } => {
                                !rhs.is_empty() && !mods.flags.contains(Flags::LAZY)
                            }
                            _ => false,
                        })
                        .cloned()
                        .collect();
                    if !vals.is_empty() && !tree.sym.is_none() {
                        self.trait_vals.insert(tree.sym, vals);
                    }
                }
                for s in &impl_.body {
                    self.collect_trait_impls(s);
                }
            }
            TreeKind::ModuleDef { impl_, .. } => {
                for s in &impl_.body {
                    self.collect_trait_impls(s);
                }
            }
            _ => {}
        }
    }

    fn emit_anon_classes(&mut self, tree: &Tree) {
        if let TreeKind::New { tpt } = &tree.kind {
            if let TreeKind::ClassDef { name, impl_, .. } = &tpt.kind {
                if name.starts_with("$anon") {
                    self.emit_class(tpt, &HashSet::new());
                    for s in &impl_.body {
                        self.emit_anon_classes(s);
                    }
                    return;
                }
            }
        }
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.emit_anon_classes(s);
                }
            }
            TreeKind::ClassDef {
                vparamss, impl_, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.emit_anon_classes(p);
                    }
                }
                for p in &impl_.parents {
                    self.emit_anon_classes(p);
                }
                for s in &impl_.body {
                    self.emit_anon_classes(s);
                }
            }
            TreeKind::ModuleDef { impl_, .. } => {
                for p in &impl_.parents {
                    self.emit_anon_classes(p);
                }
                for s in &impl_.body {
                    self.emit_anon_classes(s);
                }
            }
            TreeKind::ValDef { tpt, rhs, .. } => {
                self.emit_anon_classes(tpt);
                self.emit_anon_classes(rhs);
            }
            TreeKind::DefDef {
                vparamss, tpt, rhs, ..
            } => {
                for clause in vparamss {
                    for p in clause {
                        self.emit_anon_classes(p);
                    }
                }
                self.emit_anon_classes(tpt);
                self.emit_anon_classes(rhs);
            }
            TreeKind::Block { stats, expr } => {
                for s in stats {
                    // Local `class` / `object` declared inside a method body.
                    match &s.kind {
                        TreeKind::ClassDef { .. } => self.emit_class(s, &HashSet::new()),
                        TreeKind::ModuleDef { .. } => self.emit_module(s, &HashSet::new()),
                        _ => {}
                    }
                    self.emit_anon_classes(s);
                }
                self.emit_anon_classes(expr);
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.emit_anon_classes(cond);
                self.emit_anon_classes(thenp);
                self.emit_anon_classes(elsep);
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                self.emit_anon_classes(cond);
                self.emit_anon_classes(body);
            }
            TreeKind::Assign { lhs, rhs } => {
                self.emit_anon_classes(lhs);
                self.emit_anon_classes(rhs);
            }
            TreeKind::Match { selector, cases } => {
                self.emit_anon_classes(selector);
                for c in cases {
                    self.emit_anon_classes(&c.pat);
                    self.emit_anon_classes(&c.guard);
                    self.emit_anon_classes(&c.body);
                }
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.emit_anon_classes(p);
                }
                self.emit_anon_classes(body);
            }
            TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
                self.emit_anon_classes(fun);
                for a in args {
                    self.emit_anon_classes(a);
                }
            }
            TreeKind::Typed { expr, tpt } => {
                self.emit_anon_classes(expr);
                self.emit_anon_classes(tpt);
            }
            TreeKind::Select { qual, .. } => self.emit_anon_classes(qual),
            TreeKind::Return { expr } | TreeKind::Throw { expr } => self.emit_anon_classes(expr),
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                self.emit_anon_classes(block);
                for c in catches {
                    self.emit_anon_classes(&c.pat);
                    self.emit_anon_classes(&c.body);
                }
                self.emit_anon_classes(finalizer);
            }
            TreeKind::InterpolatedString { args, .. } => {
                for a in args {
                    self.emit_anon_classes(a);
                }
            }
            TreeKind::UnApply { fun, args } => {
                self.emit_anon_classes(fun);
                for a in args {
                    self.emit_anon_classes(a);
                }
            }
            TreeKind::LabelDef { params, rhs, .. } => {
                for p in params {
                    self.emit_anon_classes(p);
                }
                self.emit_anon_classes(rhs);
            }
            _ => {}
        }
    }

    fn walk(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => self.walk_stats(stats),
            TreeKind::ClassDef { .. } => {
                self.emit_class(tree, &HashSet::new());
            }
            TreeKind::ModuleDef { .. } => {
                self.emit_module(tree, &HashSet::new());
            }
            _ => {}
        }
    }

    fn walk_stats(&mut self, stats: &[Tree]) {
        let mut module_names = HashSet::new();
        let mut class_names = HashSet::new();
        for s in stats {
            match &s.kind {
                TreeKind::ModuleDef { name, .. } => {
                    module_names.insert(name.clone());
                }
                TreeKind::ClassDef { name, .. } => {
                    class_names.insert(name.clone());
                }
                _ => {}
            }
        }
        for s in stats {
            match &s.kind {
                TreeKind::PackageDef { .. } => self.walk(s),
                TreeKind::ClassDef {
                    name, mods, impl_, ..
                } => {
                    self.emit_class(s, &module_names);
                    if mods.flags.contains(Flags::CASE) && !module_names.contains(name) {
                        self.emit_case_companion(s);
                    }
                    self.walk_stats(&impl_.body);
                }
                TreeKind::ModuleDef { impl_, .. } => {
                    self.emit_module(s, &class_names);
                    self.walk_stats(&impl_.body);
                }
                _ => {}
            }
        }
    }

    fn emit_class(&mut self, tree: &Tree, _module_names: &HashSet<String>) {
        let (name, mods, vparamss, impl_) = match &tree.kind {
            TreeKind::ClassDef {
                name,
                mods,
                vparamss,
                impl_,
                ..
            } => (name, mods, vparamss, impl_),
            _ => return,
        };
        let class_id = tree.sym;
        let this_name = if class_id.is_none() {
            name.clone()
        } else {
            class_internal(self.st, class_id)
        };
        let is_trait = mods.flags.contains(Flags::TRAIT);
        let (super_name, interfaces) = split_parents(self.st, &impl_.parents);

        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.super_name = super_name;
        b.interfaces = interfaces;

        if is_trait {
            b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
            b.super_name = "java/lang/Object".into();
            for stt in &impl_.body {
                if let TreeKind::DefDef { name, mods, .. } = &stt.kind {
                    if name == "<init>" || name == "<clinit>" {
                        continue;
                    }
                    let acc = method_access_flags(mods.flags) | ACC_ABSTRACT;
                    b.add_abstract(acc, name, &def_method_desc(self.st, stt));
                    if needs_super_accessor(stt) {
                        let acc_name = super_accessor_name(self.st, class_id, name);
                        b.add_abstract(
                            ACC_PUBLIC | ACC_ABSTRACT,
                            &acc_name,
                            &def_method_desc(self.st, stt),
                        );
                    }
                }
                if let TreeKind::ValDef {
                    name, mods, rhs, ..
                } = &stt.kind
                {
                    let ty = val_tree_ty(self.st, stt);
                    let gdesc = format!("(){}", jvm_desc(self.st, &ty));
                    b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, name, &gdesc);
                    if !rhs.is_empty() && !mods.flags.contains(Flags::LAZY) {
                        let sdesc = format!("({})V", jvm_desc(self.st, &ty));
                        b.add_abstract(ACC_PUBLIC | ACC_ABSTRACT, &setter_name(name), &sdesc);
                    }
                }
            }
            attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
            self.out.push(b.finish());
            self.emit_trait_impl_class(tree, &this_name);
            return;
        }

        b.access = ACC_PUBLIC | ACC_SUPER;
        if mods.flags.contains(Flags::FINAL) {
            b.access |= ACC_FINAL;
        }

        // constructor / body fields
        for clause in vparamss {
            for p in clause {
                if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                    let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                        self.st.get(p.sym).ty.clone()
                    } else {
                        p.ty.clone()
                    };
                    b.fields.push(Field {
                        access: field_access_flags(mods.flags),
                        name: name.clone(),
                        desc: jvm_desc(self.st, &ty),
                    });
                }
            }
        }
        if let Some(outer) = enclosing_instance(self.st, class_id) {
            b.fields.push(Field {
                access: ACC_PUBLIC | ACC_FINAL,
                name: "$outer".into(),
                desc: format!("L{};", class_internal(self.st, outer)),
            });
        }
        // Enclosing-method locals read by the body of a class defined inside a
        // method. Public so lambdas lifted out of this class can read them.
        for (_, fname, fdesc, _) in capture_slots(self.st, &self.boxed_vars, class_id) {
            b.fields.push(Field {
                access: ACC_PUBLIC | ACC_FINAL,
                name: fname,
                desc: fdesc,
            });
        }
        for stt in &impl_.body {
            if let TreeKind::ValDef { name, mods, .. } = &stt.kind {
                let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                    self.st.get(stt.sym).ty.clone()
                } else {
                    stt.ty.clone()
                };
                b.fields.push(Field {
                    access: field_access_flags(mods.flags),
                    name: name.clone(),
                    desc: jvm_desc(self.st, &ty),
                });
            }
        }
        for (name, ty) in self.mixin_val_fields(class_id, vparamss, &impl_.body) {
            b.fields.push(Field {
                access: ACC_PUBLIC,
                name,
                desc: jvm_desc(self.st, &ty),
            });
        }
        if impl_.body.iter().any(|s| match &s.kind {
            TreeKind::ValDef { mods, .. } => mods.flags.contains(Flags::LAZY),
            _ => false,
        }) {
            b.fields.push(Field {
                access: ACC_PRIVATE,
                name: "bitmap$0".into(),
                desc: "I".into(),
            });
        }
        self.emit_class_ctor(&mut b, class_id, vparamss, &impl_.body, &impl_.parents);
        self.emit_lazy_accessors(&mut b, class_id, &impl_.body);
        self.emit_val_getters(&mut b, &impl_.body);
        for stt in &impl_.body {
            if matches!(stt.kind, TreeKind::DefDef { .. }) {
                self.emit_def(&mut b, class_id, stt);
                if self.st.is_value_class(class_id) {
                    self.emit_value_extension(&mut b, class_id, stt);
                }
            }
        }
        self.emit_default_getters(&mut b, class_id);
        self.emit_trait_val_accessors(&mut b, class_id, &impl_.body);
        self.emit_super_accessors(&mut b, class_id);
        self.emit_mixin_forwarders(&mut b, class_id, &impl_.body);
        self.emit_delayed_init_support(&mut b, class_id, &impl_.body, false);
        self.emit_case_object_methods(&mut b, class_id);
        self.emit_erasure_bridges(&mut b, class_id);
        attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
        self.out.push(b.finish());
    }

    fn delayed_body_class(class_name: &str) -> String {
        format!("{}$delayedInit$body", class_name.replace('/', "$"))
    }

    fn emit_delayed_init_support(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        body: &[Tree],
        is_module: bool,
    ) {
        if class_id.is_none() || !extends_delayed_init(self.st, class_id) {
            return;
        }
        let class_name = b.this_name.clone();
        let is_app = extends_app(self.st, class_id);
        if is_app {
            if self.library_abi {
                self.emit_app_library_members(b, &class_name, is_module);
            } else {
                self.emit_app_private_members(b, &class_name);
            }
        }
        self.emit_delayed_endpoint(b, class_id, body);
        self.emit_delayed_init_lambda(&class_name);
    }

    fn emit_app_private_members(&self, b: &mut ClassBuilder, class_name: &str) {
        let already = b.methods.iter().any(|m| m.name == "delayedInit");
        b.fields.push(Field {
            access: ACC_PRIVATE,
            name: "scala$App$$delayed".into(),
            desc: "Lscala/Function0;".into(),
        });
        if !already {
            let cn = class_name.to_string();
            b.add_code(
                ACC_PUBLIC,
                "delayedInit",
                "(Lscala/Function0;)V",
                2,
                |asm| {
                    asm.aload(0);
                    asm.aload(1);
                    asm.putfield(&cn, "scala$App$$delayed", "Lscala/Function0;");
                    asm.vreturn();
                },
            );
        }
        if !b.methods.iter().any(|m| m.name == "main") {
            let cn = class_name.to_string();
            b.add_code(ACC_PUBLIC, "main", "([Ljava/lang/String;)V", 2, |asm| {
                asm.aload(0);
                asm.getfield(&cn, "scala$App$$delayed", "Lscala/Function0;");
                let done = asm.fresh_label();
                asm.ifnull(done);
                asm.aload(0);
                asm.getfield(&cn, "scala$App$$delayed", "Lscala/Function0;");
                asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
                asm.pop();
                asm.mark(done);
                asm.vreturn();
            });
        }
    }

    fn emit_app_library_members(&self, b: &mut ClassBuilder, class_name: &str, is_module: bool) {
        let acc_f = if is_module {
            ACC_PRIVATE | ACC_STATIC
        } else {
            ACC_PRIVATE
        };
        b.fields.push(Field {
            access: acc_f,
            name: "executionStart".into(),
            desc: "J".into(),
        });
        b.fields.push(Field {
            access: acc_f,
            name: "scala$App$$_args".into(),
            desc: "[Ljava/lang/String;".into(),
        });
        b.fields.push(Field {
            access: acc_f,
            name: "scala$App$$initCode".into(),
            desc: "Lscala/collection/mutable/ListBuffer;".into(),
        });
        let cn = class_name.to_string();
        if is_module {
            b.add_code(ACC_PUBLIC, "executionStart", "()J", 1, {
                let cn = cn.clone();
                move |asm| {
                    asm.getstatic(&cn, "executionStart", "J");
                    asm.lreturn();
                }
            });
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$executionStart_$eq",
                "(J)V",
                3,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.lload(1);
                        asm.putstatic(&cn, "executionStart", "J");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args",
                "()[Ljava/lang/String;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.getstatic(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args_$eq",
                "([Ljava/lang/String;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(1);
                        asm.putstatic(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$initCode",
                "()Lscala/collection/mutable/ListBuffer;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.getstatic(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$scala$App$$initCode_$eq",
                "(Lscala/collection/mutable/ListBuffer;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(1);
                        asm.putstatic(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.vreturn();
                    }
                },
            );
        } else {
            b.add_code(ACC_PUBLIC, "executionStart", "()J", 1, {
                let cn = cn.clone();
                move |asm| {
                    asm.aload(0);
                    asm.getfield(&cn, "executionStart", "J");
                    asm.lreturn();
                }
            });
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$executionStart_$eq",
                "(J)V",
                3,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.lload(1);
                        asm.putfield(&cn, "executionStart", "J");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args",
                "()[Ljava/lang/String;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.getfield(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$_args_$eq",
                "([Ljava/lang/String;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.aload(1);
                        asm.putfield(&cn, "scala$App$$_args", "[Ljava/lang/String;");
                        asm.vreturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$$initCode",
                "()Lscala/collection/mutable/ListBuffer;",
                1,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.getfield(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.areturn();
                    }
                },
            );
            b.add_code(
                ACC_PUBLIC,
                "scala$App$_setter_$scala$App$$initCode_$eq",
                "(Lscala/collection/mutable/ListBuffer;)V",
                2,
                {
                    let cn = cn.clone();
                    move |asm| {
                        asm.aload(0);
                        asm.aload(1);
                        asm.putfield(
                            &cn,
                            "scala$App$$initCode",
                            "Lscala/collection/mutable/ListBuffer;",
                        );
                        asm.vreturn();
                    }
                },
            );
        }
        if !b.methods.iter().any(|m| m.name == "delayedInit") {
            b.add_code(
                ACC_PUBLIC,
                "delayedInit",
                "(Lscala/Function0;)V",
                2,
                |asm| {
                    asm.aload(0);
                    asm.aload(1);
                    asm.invokestatic_interface(
                        "scala/App",
                        "delayedInit$",
                        "(Lscala/App;Lscala/Function0;)V",
                    );
                    asm.vreturn();
                },
            );
        }
        if !b.methods.iter().any(|m| m.name == "main") {
            b.add_code(ACC_PUBLIC, "main", "([Ljava/lang/String;)V", 2, |asm| {
                asm.aload(0);
                asm.aload(1);
                asm.invokestatic_interface(
                    "scala/App",
                    "main$",
                    "(Lscala/App;[Ljava/lang/String;)V",
                );
                asm.vreturn();
            });
        }
        if !b.methods.iter().any(|m| m.name == "args") {
            b.add_code(ACC_PUBLIC, "args", "()[Ljava/lang/String;", 1, |asm| {
                asm.aload(0);
                asm.invokestatic_interface(
                    "scala/App",
                    "args$",
                    "(Lscala/App;)[Ljava/lang/String;",
                );
                asm.areturn();
            });
        }
    }

    fn emit_delayed_endpoint(&self, b: &mut ClassBuilder, class_id: SymbolId, body: &[Tree]) {
        let class_name = b.this_name.clone();
        let st = self.st;
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let stats: Vec<Tree> = body
            .iter()
            .filter(|t| is_delayed_ctor_stat(t) && !is_presuper_val(t))
            .cloned()
            .collect();
        b.add_code(ACC_PUBLIC, "delayedEndpoint$body", "()V", 4, |asm| {
            let mut frame = Frame::instance();
            let ctx = emit_ctx(
                st,
                class_id,
                &class_name,
                Type::Unit,
                extras,
                lambda_n,
                source,
                library_abi,
                boxed_vars,
            );
            for stt in &stats {
                if let TreeKind::ValDef {
                    name, mods, rhs, ..
                } = &stt.kind
                {
                    if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                        continue;
                    }
                    asm.aload(0);
                    gen_expr(asm, &mut frame, &ctx, rhs);
                    let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                        st.get(stt.sym).ty.clone()
                    } else {
                        stt.ty.clone()
                    };
                    asm.putfield(&class_name, name, &jvm_desc(st, &ty));
                } else {
                    gen_expr(asm, &mut frame, &ctx, stt);
                    pop_if_value(asm, &stt.ty);
                }
            }
            asm.vreturn();
        });
    }

    fn emit_delayed_init_lambda(&self, class_name: &str) {
        let lam = Self::delayed_body_class(class_name);
        let mut b = ClassBuilder::new(lam.clone(), self.source_name);
        b.access = ACC_PUBLIC | ACC_SUPER | ACC_SYNTHETIC | ACC_FINAL;
        b.interfaces.push("scala/Function0".into());
        b.fields.push(Field {
            access: ACC_PUBLIC,
            name: "$outer".into(),
            desc: format!("L{class_name};"),
        });
        let outer_d = format!("L{class_name};");
        let lam_c = lam.clone();
        let cn = class_name.to_string();
        b.add_code(ACC_PUBLIC, "<init>", &format!("({outer_d})V"), 2, |asm| {
            asm.aload(0);
            asm.invokespecial("java/lang/Object", "<init>", "()V");
            asm.aload(0);
            asm.aload(1);
            asm.putfield(&lam_c, "$outer", &outer_d);
            asm.vreturn();
        });
        b.add_code(ACC_PUBLIC, "apply", "()Ljava/lang/Object;", 1, |asm| {
            asm.aload(0);
            asm.getfield(&lam, "$outer", &format!("L{cn};"));
            asm.invokevirtual(&cn, "delayedEndpoint$body", "()V");
            asm.aconst_null();
            asm.areturn();
        });
        self.extras.borrow_mut().push(b.finish());
    }

    fn emit_delayed_init_call(asm: &mut crate::code::Assembler, class_name: &str) {
        let lam = Self::delayed_body_class(class_name);
        asm.aload(0);
        asm.new_obj(&lam);
        asm.dup();
        asm.aload(0);
        asm.invokespecial(&lam, "<init>", &format!("(L{class_name};)V"));
        asm.invokevirtual(class_name, "delayedInit", "(Lscala/Function0;)V");
    }

    fn emit_class_ctor(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        vparamss: &[Vec<Tree>],
        body: &[Tree],
        parents: &[Tree],
    ) {
        let params: Vec<&Tree> = vparamss.iter().flatten().collect();
        let mut frame = Frame::instance();
        let outer = enclosing_instance(self.st, class_id);
        let outer_desc = outer.map(|o| format!("L{};", class_internal(self.st, o)));
        if outer.is_some() {
            frame.next_slot += 1; // slot 1 is $outer
        }
        let mut param_info = Vec::new();
        for p in &params {
            let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                self.st.get(p.sym).ty.clone()
            } else {
                p.ty.clone()
            };
            let sort = jvm_sort(&ty);
            let slot = frame.alloc(p.sym, sort);
            let fname = p.name().unwrap_or("").to_string();
            param_info.push((slot, sort, fname, jvm_desc(self.st, &ty)));
        }
        let mut types: Vec<Type> = Vec::new();
        if let Some(o) = outer {
            types.push(Type::Class {
                sym: o,
                args: vec![],
            });
        }
        for p in &params {
            if p.ty.is_no_type() && !p.sym.is_none() {
                types.push(self.st.get(p.sym).ty.clone());
            } else {
                types.push(p.ty.clone());
            }
        }
        // Captures come last, after `$outer` and the source parameters.
        let caps = capture_slots(self.st, &self.boxed_vars, class_id);
        let mut cap_info = Vec::new();
        for (id, fname, fdesc, sort) in &caps {
            let slot = frame.alloc(*id, *sort);
            cap_info.push((slot, *sort, fname.clone(), fdesc.clone()));
        }
        let desc = desc_with_extra_params(
            &jvm_method_desc(self.st, &types, &Type::Unit),
            &capture_params_desc(self.st, &self.boxed_vars, class_id),
        );
        let super_name = b.super_name.clone();
        let (super_owner, super_desc, super_args) =
            parent_super_ctor(self.st, parents, &super_name);
        let class_name = b.this_name.clone();
        let st = self.st;
        let inits: Vec<&Tree> = body
            .iter()
            .filter(|t| matches!(t.kind, TreeKind::ValDef { .. }))
            .collect();
        let max_locals = frame.next_slot.max(4);
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let delayed = extends_delayed_init(st, class_id);
        let is_app = extends_app(st, class_id);
        let has_outer = outer.is_some();
        let outer_desc_c = outer_desc.clone();
        let mixin_inits: Vec<(String, String)> = if class_id.is_none() {
            Vec::new()
        } else {
            linearize(st, class_id)
                .into_iter()
                .skip(1)
                .rev()
                .filter_map(|p| {
                    if !self.trait_vals.contains_key(&p) || !is_interface_sym(st, p) {
                        return None;
                    }
                    let iface = class_internal(st, p);
                    Some((format!("{}$class", iface), format!("(L{iface};)V")))
                })
                .collect()
        };
        b.add_code(ACC_PUBLIC, "<init>", &desc, max_locals, |asm| {
            let mut frame = frame;
            let ctx_early = emit_ctx(
                st,
                class_id,
                &class_name,
                Type::Unit,
                extras,
                lambda_n,
                source,
                library_abi,
                boxed_vars,
            );
            // nsc: early vals are stored to fields before the superclass ctor so
            // parent / trait `$init$` bodies see the values.
            for vd in &inits {
                if !is_presuper_val(vd) {
                    continue;
                }
                if let TreeKind::ValDef {
                    name, mods, rhs, ..
                } = &vd.kind
                {
                    if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                        continue;
                    }
                    asm.aload(0);
                    gen_expr(asm, &mut frame, &ctx_early, rhs);
                    let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                        st.get(vd.sym).ty.clone()
                    } else {
                        vd.ty.clone()
                    };
                    asm.putfield(&class_name, name, &jvm_desc(st, &ty));
                }
            }
            asm.aload(0);
            for a in &super_args {
                gen_expr(asm, &mut frame, &ctx_early, a);
            }
            asm.invokespecial(&super_owner, "<init>", &super_desc);
            if has_outer {
                if let Some(od) = &outer_desc_c {
                    asm.aload(0);
                    asm.aload(1);
                    asm.putfield(&class_name, "$outer", od);
                }
            }
            for (slot, sort, fname, fdesc) in &param_info {
                if fname.is_empty() {
                    continue;
                }
                asm.aload(0);
                load(asm, *slot, *sort);
                asm.putfield(&class_name, fname, fdesc);
            }
            for (slot, sort, fname, fdesc) in &cap_info {
                asm.aload(0);
                load(asm, *slot, *sort);
                asm.putfield(&class_name, fname, fdesc);
            }
            for (impl_cls, init_desc) in &mixin_inits {
                asm.aload(0);
                asm.invokestatic(impl_cls, "$init$", init_desc);
            }
            let ctx = emit_ctx(
                st,
                class_id,
                &class_name,
                Type::Unit,
                extras,
                lambda_n,
                source,
                library_abi,
                boxed_vars,
            );
            if delayed {
                if library_abi && is_app {
                    asm.aload(0);
                    asm.invokestatic_interface("scala/App", "$init$", "(Lscala/App;)V");
                }
                Gen::emit_delayed_init_call(asm, &class_name);
            } else {
                for vd in &inits {
                    if is_presuper_val(vd) {
                        continue;
                    }
                    if let TreeKind::ValDef {
                        name, mods, rhs, ..
                    } = &vd.kind
                    {
                        if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                            continue;
                        }
                        asm.aload(0);
                        gen_expr(asm, &mut frame, &ctx, rhs);
                        let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                            st.get(vd.sym).ty.clone()
                        } else {
                            vd.ty.clone()
                        };
                        asm.putfield(&class_name, name, &jvm_desc(st, &ty));
                    }
                }
            }
            asm.vreturn();
        });
    }

    fn emit_def(&self, b: &mut ClassBuilder, class_id: SymbolId, def: &Tree) {
        let (name, mods, vparamss, rhs) = match &def.kind {
            TreeKind::DefDef {
                name,
                mods,
                vparamss,
                rhs,
                ..
            } => (name, mods, vparamss, rhs),
            _ => return,
        };
        if name == "<clinit>" {
            return;
        }
        if name == "<init>" && rhs.is_empty() {
            return;
        }
        let desc = def_method_desc_boxed(self.st, def, &self.boxed_vars);
        let ret = method_ret_ty(def);
        let acc = method_access_flags(mods.flags);
        if mods.flags.contains(Flags::NATIVE) {
            b.add_abstract(acc, name, &desc);
            if let Some(d) = java_deprecated_desc(mods) {
                b.add_java_annot_to_last(d);
            }
            return;
        }
        if rhs.is_empty() {
            b.add_abstract(acc | ACC_ABSTRACT, name, &desc);
            if let Some(d) = java_deprecated_desc(mods) {
                b.add_java_annot_to_last(d);
            }
            return;
        }
        let mut frame = Frame::instance();
        for clause in vparamss {
            for p in clause {
                let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                    self.st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                let sort = if def_is_synthetic(self.st, def)
                    && !p.sym.is_none()
                    && self.boxed_vars.contains(&p.sym)
                {
                    JvmSort::Ref
                } else {
                    jvm_sort(&ty)
                };
                frame.alloc(p.sym, sort);
            }
        }
        let class_name = b.this_name.clone();
        let st = self.st;
        let max_locals = frame.next_slot;
        let ret_for_body = ret.clone();
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let meth = def.sym;
        let caps = if acc & ACC_STATIC == 0 {
            capture_slots(self.st, &self.boxed_vars, class_id)
        } else {
            CaptureSlots::new()
        };
        b.add_code(acc, name, &desc, max_locals, |asm| {
            let mut frame = frame;
            emit_capture_prologue(asm, &mut frame, &class_name, &caps);
            let mut ctx = emit_ctx(
                st,
                class_id,
                &class_name,
                ret_for_body.clone(),
                extras,
                lambda_n,
                source,
                library_abi,
                boxed_vars,
            );
            ctx.method_sym = meth;
            finish_method_body(asm, &mut frame, &ctx, rhs, &ret_for_body);
        });
        if let Some(d) = java_deprecated_desc(mods) {
            b.add_java_annot_to_last(d);
        }
    }

    fn emit_value_extension(&self, b: &mut ClassBuilder, class_id: SymbolId, def: &Tree) {
        let (name, vparamss, rhs) = match &def.kind {
            TreeKind::DefDef {
                name,
                vparamss,
                rhs,
                ..
            } => (name, vparamss, rhs),
            _ => return,
        };
        if rhs.is_empty() || name == "<init>" || name == "<clinit>" {
            return;
        }
        let Some(under) = self.st.value_class_underlying(class_id) else {
            return;
        };
        let field = self.st.get(class_id).ctor_fields.first().copied();
        let ext_name = format!("{name}$extension");
        let desc = value_extension_desc(self.st, def.sym);
        let ret = method_ret_ty(def);
        let mut frame = Frame {
            locals: HashMap::new(),
            next_slot: 0,
        };
        if let Some(fid) = field {
            frame.alloc(fid, jvm_sort(&under));
        } else {
            frame.next_slot = 1;
        }
        for clause in vparamss {
            for p in clause {
                let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                    self.st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                frame.alloc(p.sym, jvm_sort(&ty));
            }
        }
        let class_name = b.this_name.clone();
        let st = self.st;
        let max_locals = frame.next_slot.max(1);
        let ret_for_body = ret.clone();
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        b.add_code(
            ACC_PUBLIC | ACC_STATIC,
            &ext_name,
            &desc,
            max_locals,
            |asm| {
                let mut frame = frame;
                let ctx = emit_ctx(
                    st,
                    class_id,
                    &class_name,
                    ret_for_body.clone(),
                    extras,
                    lambda_n,
                    source,
                    library_abi,
                    boxed_vars,
                );
                gen_expr(asm, &mut frame, &ctx, rhs);
                if is_unit_like(&ret_for_body) {
                    pop_if_value(asm, &rhs.ty);
                    asm.vreturn();
                } else {
                    emit_return(asm, &ret_for_body);
                }
            },
        );
    }

    fn emit_trait_impl_class(&mut self, tree: &Tree, iface: &str) {
        let class_id = tree.sym;
        let methods = self.trait_impls.get(&class_id).cloned().unwrap_or_default();
        let vals = self.trait_vals.get(&class_id).cloned().unwrap_or_default();
        if methods.is_empty() && vals.is_empty() {
            return;
        }
        let impl_name = format!("{}$class", iface);
        let mut b = ClassBuilder::new(impl_name, self.source_name);
        b.access = ACC_PUBLIC | ACC_SUPER | ACC_FINAL;
        for def in &methods {
            self.emit_trait_impl_method(&mut b, class_id, iface, def);
        }
        if !vals.is_empty() {
            self.emit_trait_init(&mut b, class_id, iface, &vals);
        }
        self.out.push(b.finish());
    }

    fn emit_trait_init(
        &self,
        b: &mut ClassBuilder,
        trait_id: SymbolId,
        iface: &str,
        vals: &[Tree],
    ) {
        let desc = format!("(L{iface};)V");
        let iface_owned = iface.to_string();
        let st = self.st;
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let vals = vals.to_vec();
        b.add_code(ACC_PUBLIC | ACC_STATIC, "$init$", &desc, 4, |asm| {
            let mut frame = Frame::instance();
            let ctx = emit_ctx(
                st,
                trait_id,
                &iface_owned,
                Type::Unit,
                extras,
                lambda_n,
                source,
                library_abi,
                boxed_vars,
            );
            for vd in &vals {
                if let TreeKind::ValDef {
                    name, mods, rhs, ..
                } = &vd.kind
                {
                    if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                        continue;
                    }
                    asm.aload(0);
                    gen_expr(asm, &mut frame, &ctx, rhs);
                    let ty = val_tree_ty(st, vd);
                    asm.invokeinterface(
                        &iface_owned,
                        &setter_name(name),
                        &format!("({})V", jvm_desc(st, &ty)),
                    );
                }
            }
            asm.vreturn();
        });
    }

    fn emit_trait_impl_method(
        &self,
        b: &mut ClassBuilder,
        trait_id: SymbolId,
        iface: &str,
        def: &Tree,
    ) {
        let (name, vparamss, rhs) = match &def.kind {
            TreeKind::DefDef {
                name,
                vparamss,
                rhs,
                ..
            } => (name, vparamss, rhs),
            _ => return,
        };
        if rhs.is_empty() {
            return;
        }
        let inst_desc = def_method_desc(self.st, def);
        let desc = trait_static_desc(iface, &inst_desc);
        let ret = method_ret_ty(def);
        let mut frame = Frame::instance(); // slot 0 = $this
        for clause in vparamss {
            for p in clause {
                let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                    self.st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                frame.alloc(p.sym, jvm_sort(&ty));
            }
        }
        let iface_owned = iface.to_string();
        let st = self.st;
        let max_locals = frame.next_slot;
        let ret_for_body = ret.clone();
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let meth = def.sym;
        b.add_code(ACC_PUBLIC | ACC_STATIC, name, &desc, max_locals, |asm| {
            let mut frame = frame;
            let mut ctx = emit_ctx(
                st,
                trait_id,
                &iface_owned,
                ret_for_body.clone(),
                extras,
                lambda_n,
                source,
                library_abi,
                boxed_vars,
            );
            ctx.method_sym = meth;
            finish_method_body(asm, &mut frame, &ctx, rhs, &ret_for_body);
        });
    }

    fn mixin_val_fields(
        &self,
        class_id: SymbolId,
        vparamss: &[Vec<Tree>],
        body: &[Tree],
    ) -> Vec<(String, Type)> {
        let mut have = HashSet::new();
        for clause in vparamss {
            for p in clause {
                if let Some(n) = p.name() {
                    have.insert(n.to_string());
                }
            }
        }
        for stt in body {
            if let TreeKind::ValDef { name, .. } = &stt.kind {
                have.insert(name.clone());
            }
        }
        let mut out = Vec::new();
        if class_id.is_none() {
            return out;
        }
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            let Some(vals) = self.trait_vals.get(&parent) else {
                continue;
            };
            for v in vals {
                let name = v.name().unwrap_or("").to_string();
                if name.is_empty() || !have.insert(name.clone()) {
                    continue;
                }
                out.push((name, val_tree_ty(self.st, v)));
            }
        }
        out
    }

    fn emit_trait_val_accessors(&self, b: &mut ClassBuilder, class_id: SymbolId, body: &[Tree]) {
        if class_id.is_none() {
            return;
        }
        let mut skip = HashSet::new();
        for stt in body {
            if let TreeKind::DefDef { name, .. } = &stt.kind {
                skip.insert(name.clone());
            }
        }
        for m in &b.methods {
            skip.insert(m.name.clone());
        }
        let mut needed: Vec<(String, Type)> = Vec::new();
        let mut seen = HashSet::new();
        for parent in linearize(self.st, class_id).into_iter().skip(1) {
            let Some(vals) = self.trait_vals.get(&parent) else {
                continue;
            };
            for v in vals {
                let name = v.name().unwrap_or("").to_string();
                if name.is_empty() || !seen.insert(name.clone()) {
                    continue;
                }
                needed.push((name, val_tree_ty(self.st, v)));
            }
        }
        let class_name = b.this_name.clone();
        for (name, ty) in needed {
            if skip.contains(&name) {
                continue;
            }
            let fdesc = jvm_desc(self.st, &ty);
            let gdesc = format!("(){fdesc}");
            let sdesc = format!("({fdesc})V");
            let fname = name.clone();
            let class_c = class_name.clone();
            let fdesc_c = fdesc.clone();
            let sort = jvm_sort(&ty);
            b.add_code(ACC_PUBLIC, &name, &gdesc, 1, |asm| {
                asm.aload(0);
                asm.getfield(&class_c, &fname, &fdesc_c);
                emit_return(asm, &ty);
            });
            let fname = name.clone();
            let class_c = class_name.clone();
            let fdesc_c = fdesc.clone();
            b.add_code(
                ACC_PUBLIC,
                &setter_name(&name),
                &sdesc,
                1 + sort.slots(),
                |asm| {
                    asm.aload(0);
                    load(asm, 1, sort);
                    asm.putfield(&class_c, &fname, &fdesc_c);
                    asm.vreturn();
                },
            );
        }
    }

    fn emit_super_accessors(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() {
            return;
        }
        let lin = linearize(self.st, class_id);
        for (idx, parent) in lin.iter().enumerate() {
            if idx == 0 || !is_interface_sym(self.st, *parent) {
                continue;
            }
            let Some(methods) = self.trait_impls.get(parent) else {
                continue;
            };
            for m in methods {
                if !needs_super_accessor(m) {
                    continue;
                }
                let name = m.name().unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                let acc = super_accessor_name(self.st, *parent, &name);
                let inst_desc = def_method_desc(self.st, m);
                let ret = method_ret_ty(m);
                let pts = def_param_types(self.st, m);
                let mut locals = 1u16;
                let mut loads = Vec::new();
                for p in &pts {
                    let sort = jvm_sort(p);
                    loads.push((locals, sort));
                    locals += sort.slots();
                }
                let target = self.next_lin_impl(&lin, idx, &name);
                let acc_c = acc.clone();
                let inst_c = inst_desc.clone();
                b.add_code(ACC_PUBLIC, &acc_c, &inst_c, locals.max(1), |asm| {
                    asm.aload(0);
                    for (slot, sort) in &loads {
                        load(asm, *slot, *sort);
                    }
                    match target {
                        Some((next, true)) => {
                            let iface = class_internal(self.st, next);
                            let static_desc = trait_static_desc(&iface, &inst_c);
                            asm.invokestatic(&format!("{}$class", iface), &name, &static_desc);
                        }
                        Some((next, false)) => {
                            let owner = class_internal(self.st, next);
                            asm.invokespecial(&owner, &name, &inst_c);
                        }
                        None => {
                            throw_runtime(asm, &format!("no super implementation for {name}"));
                            if !is_unit_like(&ret) {
                                push_default(asm, &ret);
                            }
                        }
                    }
                    emit_return(asm, &ret);
                });
            }
        }
    }

    fn next_lin_impl(
        &self,
        lin: &[SymbolId],
        after_idx: usize,
        method: &str,
    ) -> Option<(SymbolId, bool)> {
        for &s in lin.iter().skip(after_idx + 1) {
            if let Some(ms) = self.trait_impls.get(&s) {
                if ms.iter().any(|m| m.name() == Some(method)) {
                    return Some((s, true));
                }
            }
            if !is_interface_sym(self.st, s) {
                let has = self.st.get(s).members.iter().any(|&mid| {
                    let mem = self.st.get(mid);
                    mem.name == method
                        && mem.kind == SymKind::Method
                        && !mem.flags.contains(Flags::ABSTRACT)
                });
                if has {
                    return Some((s, false));
                }
            }
        }
        None
    }

    fn emit_mixin_forwarders(&self, b: &mut ClassBuilder, class_id: SymbolId, body: &[Tree]) {
        if class_id.is_none() {
            return;
        }
        let mut defined = HashSet::new();
        for stt in body {
            if let TreeKind::DefDef { name, .. } = &stt.kind {
                defined.insert(name.clone());
            }
        }
        for m in &b.methods {
            defined.insert(m.name.clone());
        }
        let lin = linearize(self.st, class_id);
        let mut chosen: Vec<(String, String, Tree)> = Vec::new();
        let mut seen = HashSet::new();
        for parent in lin.iter().skip(1) {
            let Some(methods) = self.trait_impls.get(parent) else {
                continue;
            };
            if !is_interface_sym(self.st, *parent) {
                continue;
            }
            let iface = class_internal(self.st, *parent);
            for m in methods {
                let name = m.name().unwrap_or("").to_string();
                if name.is_empty() || !seen.insert(name.clone()) {
                    continue;
                }
                chosen.push((name, iface.clone(), m.clone()));
            }
        }
        for (name, iface, def) in chosen {
            if defined.contains(&name) {
                continue;
            }
            let inst_desc = def_method_desc(self.st, &def);
            let static_desc = trait_static_desc(&iface, &inst_desc);
            let ret = method_ret_ty(&def);
            let pts = def_param_types(self.st, &def);
            let mut locals = 1u16;
            let mut loads = Vec::new();
            for p in &pts {
                let sort = jvm_sort(p);
                loads.push((locals, sort));
                locals += sort.slots();
            }
            let impl_class = format!("{}$class", iface);
            let name_c = name.clone();
            let inst_c = inst_desc.clone();
            let static_c = static_desc.clone();
            let impl_c = impl_class.clone();
            b.add_code(ACC_PUBLIC, &name_c, &inst_c, locals.max(1), |asm| {
                asm.aload(0);
                for (slot, sort) in &loads {
                    load(asm, *slot, *sort);
                }
                asm.invokestatic(&impl_c, &name_c, &static_c);
                emit_return(asm, &ret);
            });
        }
        if !self.library_abi {
            self.emit_ordered_forwarders(b, class_id, &defined);
        }
    }

    /// nsc-style erasure bridges: `compare(that: Box)` does not satisfy
    /// `Ordered.compare(Object)`. Emit a public bridge that checkcasts.
    /// A case class's `toString` / `equals` / `hashCode` / `canEqual`. nsc
    /// synthesizes these from the constructor fields; a hand-written one wins.
    /// `hashCode` folds with 31 rather than nsc's MurmurHash3, so it agrees
    /// with `equals` without depending on `scala.runtime`.
    fn emit_case_object_methods(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() || !self.st.get(class_id).flags.contains(Flags::CASE) {
            return;
        }
        let fields = self.st.get(class_id).ctor_fields.clone();
        let class_jvm = b.this_name.clone();
        let simple = self.st.get(class_id).name.clone();
        let defined: HashSet<String> = b.methods.iter().map(|m| m.name.clone()).collect();
        let field_info: Vec<(String, Type, String)> = fields
            .iter()
            .map(|f| {
                let s = self.st.get(*f);
                let ty = s.ty.clone();
                let desc = jvm_desc(self.st, &ty);
                (s.name.clone(), ty, desc)
            })
            .collect();

        if !defined.contains("toString") {
            let fi = field_info.clone();
            let cj = class_jvm.clone();
            let head = format!("{simple}(");
            b.add_code(ACC_PUBLIC, "toString", "()Ljava/lang/String;", 1, |asm| {
                asm.new_obj("java/lang/StringBuilder");
                asm.dup();
                asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
                append_str(asm, &head);
                for (i, (name, ty, desc)) in fi.iter().enumerate() {
                    if i > 0 {
                        append_str(asm, ",");
                    }
                    asm.aload(0);
                    asm.getfield(&cj, name, desc);
                    let ad = append_desc(ty);
                    if ad == "(Ljava/lang/Object;)Ljava/lang/StringBuilder;" && is_jvm_primitive(ty)
                    {
                        emit_box(asm, ty);
                    }
                    asm.invokevirtual("java/lang/StringBuilder", "append", ad);
                }
                append_str(asm, ")");
                asm.invokevirtual(
                    "java/lang/StringBuilder",
                    "toString",
                    "()Ljava/lang/String;",
                );
                asm.areturn();
            });
        }

        if !defined.contains("canEqual") {
            let cj = class_jvm.clone();
            b.add_code(ACC_PUBLIC, "canEqual", "(Ljava/lang/Object;)Z", 2, |asm| {
                asm.aload(1);
                asm.instanceof(&cj);
                asm.ireturn();
            });
        }

        if !defined.contains("equals") {
            let fi = field_info.clone();
            let cj = class_jvm.clone();
            b.add_code(ACC_PUBLIC, "equals", "(Ljava/lang/Object;)Z", 3, |asm| {
                let yes = asm.fresh_label();
                let no = asm.fresh_label();
                asm.aload(0);
                asm.aload(1);
                asm.if_acmpeq(yes);
                asm.aload(1);
                asm.instanceof(&cj);
                asm.ifeq(no);
                asm.aload(1);
                asm.checkcast(&cj);
                asm.astore(2);
                for (name, ty, desc) in &fi {
                    asm.aload(0);
                    asm.getfield(&cj, name, desc);
                    asm.aload(2);
                    asm.getfield(&cj, name, desc);
                    match ty {
                        Type::Long => {
                            asm.lcmp();
                            asm.ifne(no);
                        }
                        Type::Double => {
                            asm.dcmpl();
                            asm.ifne(no);
                        }
                        Type::Float => {
                            asm.fcmpl();
                            asm.ifne(no);
                        }
                        t if is_jvm_primitive(t) => {
                            let eq = asm.fresh_label();
                            asm.if_icmpeq(eq);
                            asm.goto(no);
                            asm.mark(eq);
                        }
                        _ => {
                            asm.invokestatic(
                                "java/util/Objects",
                                "equals",
                                "(Ljava/lang/Object;Ljava/lang/Object;)Z",
                            );
                            asm.ifeq(no);
                        }
                    }
                }
                asm.mark(yes);
                asm.iconst(1);
                asm.ireturn();
                asm.mark(no);
                asm.iconst(0);
                asm.ireturn();
            });
        }

        if !defined.contains("hashCode") {
            let fi = field_info.clone();
            let cj = class_jvm.clone();
            b.add_code(ACC_PUBLIC, "hashCode", "()I", 2, |asm| {
                asm.iconst(0);
                for (name, ty, desc) in &fi {
                    asm.iconst(31);
                    asm.imul();
                    asm.aload(0);
                    asm.getfield(&cj, name, desc);
                    if is_jvm_primitive(ty) {
                        emit_box(asm, ty);
                    }
                    asm.invokestatic("java/util/Objects", "hashCode", "(Ljava/lang/Object;)I");
                    asm.iadd();
                }
                asm.ireturn();
            });
        }
    }

    fn emit_erasure_bridges(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() {
            return;
        }
        let class_name = b.this_name.clone();
        let existing: HashSet<(String, String)> = b
            .methods
            .iter()
            .map(|m| (m.name.clone(), m.desc.clone()))
            .collect();
        let own: Vec<(String, SymbolId)> = self
            .st
            .get(class_id)
            .members
            .iter()
            .copied()
            .filter(|&id| self.st.get(id).kind == SymKind::Method)
            .map(|id| (self.st.get(id).name.clone(), id))
            .collect();
        let lin = linearize(self.st, class_id);
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for parent in lin.into_iter().skip(1) {
            for pmid in self.st.get(parent).members.clone() {
                let ps = self.st.get(pmid);
                if ps.kind != SymKind::Method {
                    continue;
                }
                if ps.name == "<init>" || ps.name == "<clinit>" {
                    continue;
                }
                let Some((_, cid)) = own.iter().find(|(n, _)| n == &ps.name) else {
                    continue;
                };
                if *cid == pmid {
                    continue;
                }
                let pdesc = method_desc_from_sym(self.st, pmid);
                let cdesc = method_desc_from_sym(self.st, *cid);
                if pdesc == cdesc {
                    continue;
                }
                let enc = encode_method_name(&ps.name);
                if existing.contains(&(enc.clone(), pdesc.clone())) {
                    continue;
                }
                if !seen.insert((enc.clone(), pdesc.clone())) {
                    continue;
                }
                let parent_params = method_params_from_sym(self.st, pmid);
                let child_params = method_params_from_sym(self.st, *cid);
                if parent_params.len() != child_params.len() {
                    continue;
                }
                let ret = method_ret_from_sym(self.st, pmid);
                let child_ret = method_ret_from_sym(self.st, *cid);
                // The bridge takes the erased parent signature, so a parameter
                // the subclass narrowed to a primitive arrives boxed.
                let ret_adapt = if jvm_desc(self.st, &ret) == jvm_desc(self.st, &child_ret) {
                    Adapt::None
                } else {
                    param_adapt(self.st, &child_ret, &ret)
                };
                let mut locals = 1u16;
                let mut loads = Vec::new();
                let mut casts: Vec<Adapt> = Vec::new();
                for (pty, cty) in parent_params.iter().zip(child_params.iter()) {
                    let sort = jvm_sort(pty);
                    loads.push((locals, sort));
                    let adapt = if jvm_desc(self.st, pty) != jvm_desc(self.st, cty) {
                        param_adapt(self.st, pty, cty)
                    } else {
                        Adapt::None
                    };
                    casts.push(adapt);
                    locals += sort.slots();
                }
                let name = ps.name.clone();
                let pdesc_c = pdesc.clone();
                let cdesc_c = cdesc.clone();
                let class_c = class_name.clone();
                b.add_code(
                    ACC_PUBLIC | ACC_SYNTHETIC | ACC_BRIDGE,
                    &name,
                    &pdesc_c,
                    locals.max(1),
                    |asm| {
                        asm.aload(0);
                        for (i, (slot, sort)) in loads.iter().enumerate() {
                            load(asm, *slot, *sort);
                            if let Some(a) = casts.get(i) {
                                emit_adapt(asm, a);
                            }
                        }
                        asm.invokevirtual(&class_c, &name, &cdesc_c);
                        emit_adapt(asm, &ret_adapt);
                        emit_return(asm, &ret);
                    },
                );
            }
        }
    }

    fn emit_ordered_forwarders(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        defined: &HashSet<String>,
    ) {
        if class_id.is_none() {
            return;
        }
        let lin = linearize(self.st, class_id);
        let has_ordered = lin.iter().any(|&p| self.st.get(p).name == "Ordered");
        if !has_ordered {
            return;
        }
        for op in ["<", ">", "<=", ">="] {
            let enc = encode_method_name(op);
            if defined.contains(op) || defined.contains(&enc) {
                continue;
            }
            let desc = "(Ljava/lang/Object;)Z";
            let static_desc = "(Lscala/math/Ordered;Ljava/lang/Object;)Z";
            let name = op.to_string();
            b.add_code(ACC_PUBLIC, &name, desc, 2, |asm| {
                asm.aload(0);
                asm.aload(1);
                asm.invokestatic("scala/math/Ordered$class", &name, static_desc);
                asm.ireturn();
            });
        }
    }

    fn emit_default_getters(&self, b: &mut ClassBuilder, class_id: SymbolId) {
        if class_id.is_none() {
            return;
        }
        let existing: HashSet<String> = b.methods.iter().map(|m| m.name.clone()).collect();
        for mid in self.st.get(class_id).members.clone() {
            let s = self.st.get(mid);
            if s.kind != SymKind::Method || !s.name.contains("$default$") {
                continue;
            }
            if existing.contains(&encode_method_name(&s.name)) {
                continue;
            }
            let Some(rhs) = s.default_rhs.clone() else {
                continue;
            };
            let name = s.name.clone();
            let pts: Vec<Type> = if !s.params.is_empty() {
                s.params
                    .iter()
                    .map(|p| self.st.get(*p).ty.clone())
                    .collect()
            } else {
                match &s.ty {
                    Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
                    _ => vec![],
                }
            };
            let ret = match &s.ty {
                Type::Method { ret, .. } => (**ret).clone(),
                _ => rhs.ty.clone(),
            };
            let desc = jvm_method_desc(self.st, &pts, &ret);
            let mut frame = Frame::instance();
            let pids = s.params.clone();
            for (i, ty) in pts.iter().enumerate() {
                let id = pids.get(i).copied().unwrap_or(SymbolId::NONE);
                frame.alloc(id, jvm_sort(ty));
            }
            let class_name = b.this_name.clone();
            let st = self.st;
            let max_locals = frame.next_slot.max(1);
            let extras = &self.extras;
            let lambda_n = &self.lambda_n;
            let source = self.source_name;
            let library_abi = self.library_abi;
            let boxed_vars = &self.boxed_vars;
            let ret_for_body = ret.clone();
            b.add_code(
                ACC_PUBLIC | ACC_SYNTHETIC,
                &name,
                &desc,
                max_locals,
                |asm| {
                    let mut frame = frame;
                    let ctx = emit_ctx(
                        st,
                        class_id,
                        &class_name,
                        ret_for_body.clone(),
                        extras,
                        lambda_n,
                        source,
                        library_abi,
                        boxed_vars,
                    );
                    gen_expr(asm, &mut frame, &ctx, &rhs);
                    if is_unit_like(&ret_for_body) {
                        pop_if_value(asm, &rhs.ty);
                        asm.vreturn();
                    } else {
                        emit_return(asm, &ret_for_body);
                    }
                },
            );
        }
    }

    fn emit_lazy_accessors(&self, b: &mut ClassBuilder, class_id: SymbolId, body: &[Tree]) {
        let mut bit = 0i32;
        for stt in body {
            let TreeKind::ValDef {
                name, mods, rhs, ..
            } = &stt.kind
            else {
                continue;
            };
            if !mods.flags.contains(Flags::LAZY) || rhs.is_empty() {
                continue;
            }
            let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                self.st.get(stt.sym).ty.clone()
            } else {
                stt.ty.clone()
            };
            let desc = format!("(){}", jvm_desc(self.st, &ty));
            let class_name = b.this_name.clone();
            let fname = name.clone();
            let fdesc = jvm_desc(self.st, &ty);
            let st = self.st;
            let extras = &self.extras;
            let lambda_n = &self.lambda_n;
            let source = self.source_name;
            let library_abi = self.library_abi;
            let boxed_vars = &self.boxed_vars;
            let rhs = rhs.clone();
            let mask = 1i32 << bit;
            bit += 1;
            let ret_ty = ty.clone();
            let caps = capture_slots(self.st, &self.boxed_vars, class_id);
            b.add_code(ACC_PUBLIC, &fname, &desc, 4, |asm| {
                let mut frame = Frame::instance();
                emit_capture_prologue(asm, &mut frame, &class_name, &caps);
                let lock = frame.alloc_tmp(JvmSort::Ref);
                let result = frame.alloc_tmp(jvm_sort(&ret_ty));
                asm.aload(0);
                store(asm, lock, JvmSort::Ref);
                load(asm, lock, JvmSort::Ref);
                asm.monitorenter();
                asm.aload(0);
                asm.getfield(&class_name, "bitmap$0", "I");
                asm.iconst(mask);
                asm.iand();
                let inited = asm.fresh_label();
                asm.ifne(inited);
                let ctx = emit_ctx(
                    st,
                    class_id,
                    &class_name,
                    ret_ty.clone(),
                    extras,
                    lambda_n,
                    source,
                    library_abi,
                    boxed_vars,
                );
                asm.aload(0);
                gen_expr(asm, &mut frame, &ctx, &rhs);
                asm.putfield(&class_name, &fname, &fdesc);
                asm.aload(0);
                asm.aload(0);
                asm.getfield(&class_name, "bitmap$0", "I");
                asm.iconst(mask);
                asm.ior();
                asm.putfield(&class_name, "bitmap$0", "I");
                asm.mark(inited);
                asm.aload(0);
                asm.getfield(&class_name, &fname, &fdesc);
                store(asm, result, jvm_sort(&ret_ty));
                load(asm, lock, JvmSort::Ref);
                asm.monitorexit();
                load(asm, result, jvm_sort(&ret_ty));
                emit_return(asm, &ret_ty);
            });
        }
    }

    /// nsc-style val getters (`def Red: Value`) so `scala.Enumeration` reflection
    /// (`populateNameMap` / `isValDef`) can pair method `Red()` with field `Red`.
    fn emit_val_getters(&self, b: &mut ClassBuilder, body: &[Tree]) {
        let class_name = b.this_name.clone();
        for stt in body {
            let TreeKind::ValDef { name, mods, .. } = &stt.kind else {
                continue;
            };
            if mods.flags.contains(Flags::LAZY) {
                continue;
            }
            let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                self.st.get(stt.sym).ty.clone()
            } else {
                stt.ty.clone()
            };
            if ty.is_no_type() || ty.is_error() {
                continue;
            }
            let desc = format!("(){}", jvm_desc(self.st, &ty));
            let fname = name.clone();
            let fdesc = jvm_desc(self.st, &ty);
            let ret_ty = ty.clone();
            b.add_code(ACC_PUBLIC, &fname, &desc, 1, |asm| {
                asm.aload(0);
                asm.getfield(&class_name, &fname, &fdesc);
                emit_return(asm, &ret_ty);
            });
        }
    }

    fn emit_module(&mut self, tree: &Tree, class_names: &HashSet<String>) {
        let (name, impl_) = match &tree.kind {
            TreeKind::ModuleDef { name, impl_, .. } => (name, impl_),
            _ => return,
        };
        let m = tree.sym;
        let cls = if m.is_none() {
            m
        } else {
            module_class_id(self.st, m)
        };
        let this_name = if cls.is_none() {
            format!("{name}$")
        } else {
            class_internal(self.st, cls)
        };

        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        let (super_name, interfaces) = split_parents(self.st, &impl_.parents);
        b.super_name = super_name;
        b.interfaces = interfaces;
        b.fields.push(Field {
            access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
            name: "MODULE$".into(),
            desc: format!("L{this_name};"),
        });
        for stt in &impl_.body {
            if let TreeKind::ValDef { name, mods, .. } = &stt.kind {
                let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                    self.st.get(stt.sym).ty.clone()
                } else {
                    stt.ty.clone()
                };
                b.fields.push(Field {
                    access: field_access_flags(mods.flags),
                    name: name.clone(),
                    desc: jvm_desc(self.st, &ty),
                });
            }
        }
        if impl_.body.iter().any(|s| match &s.kind {
            TreeKind::ValDef { mods, .. } => mods.flags.contains(Flags::LAZY),
            _ => false,
        }) {
            b.fields.push(Field {
                access: ACC_PRIVATE,
                name: "bitmap$0".into(),
                desc: "I".into(),
            });
        }

        self.emit_module_init(&mut b, cls, &impl_.body);
        self.emit_module_clinit(&mut b);
        self.emit_lazy_accessors(&mut b, cls, &impl_.body);
        self.emit_val_getters(&mut b, &impl_.body);

        let mut forwarded: Vec<(String, String, Type, Vec<Type>)> = Vec::new();
        for stt in &impl_.body {
            if matches!(stt.kind, TreeKind::DefDef { .. }) {
                self.emit_def(&mut b, cls, stt);
                if let TreeKind::DefDef { name, mods, .. } = &stt.kind {
                    if !mods.flags.contains(Flags::PRIVATE) && !mods.flags.contains(Flags::NATIVE) {
                        forwarded.push((
                            name.clone(),
                            def_method_desc(self.st, stt),
                            method_ret_ty(stt),
                            def_param_types(self.st, stt),
                        ));
                    }
                }
            }
        }
        // An `object` mixing in a trait needs the same `T$class` forwarders a
        // class gets, or its concrete trait methods stay abstract.
        self.emit_mixin_forwarders(&mut b, cls, &impl_.body);
        self.emit_delayed_init_support(&mut b, cls, &impl_.body, true);
        if !cls.is_none()
            && extends_app(self.st, cls)
            && !forwarded.iter().any(|(n, _, _, _)| n == "main")
        {
            forwarded.push((
                "main".into(),
                "([Ljava/lang/String;)V".into(),
                Type::Unit,
                vec![Type::Array(Box::new(Type::String))],
            ));
        }
        self.emit_default_getters(&mut b, cls);
        if !cls.is_none() {
            for mid in self.st.get(cls).members.clone() {
                let s = self.st.get(mid);
                if s.kind != SymKind::Method || !s.name.contains("$default$") {
                    continue;
                }
                if forwarded.iter().any(|(n, _, _, _)| n == &s.name) {
                    continue;
                }
                let pts: Vec<Type> = if !s.params.is_empty() {
                    s.params
                        .iter()
                        .map(|p| self.st.get(*p).ty.clone())
                        .collect()
                } else {
                    match &s.ty {
                        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
                        _ => vec![],
                    }
                };
                let ret = match &s.ty {
                    Type::Method { ret, .. } => (**ret).clone(),
                    _ => Type::Any,
                };
                forwarded.push((
                    s.name.clone(),
                    jvm_method_desc(self.st, &pts, &ret),
                    ret,
                    pts,
                ));
            }
        }

        // case-class companion: synthetic apply
        if let Some(class_id) = self.find_class_named(name) {
            if self.st.get(class_id).flags.contains(Flags::CASE)
                && !impl_.body.iter().any(|t| t.name() == Some("apply"))
            {
                emit_case_apply(&mut b, self.st, class_id);
                let fields = self.st.get(class_id).ctor_fields.clone();
                let pts: Vec<Type> = fields.iter().map(|f| self.st.get(*f).ty.clone()).collect();
                let ret = Type::Class {
                    sym: class_id,
                    args: vec![],
                };
                forwarded.push((
                    "apply".into(),
                    jvm_method_desc(self.st, &pts, &ret),
                    ret,
                    pts,
                ));
            }
        }

        attach_scala_sig(&mut b, self.st, cls, &self.pickles);
        self.out.push(b.finish());

        let top_level = if cls.is_none() {
            true
        } else {
            matches!(
                self.st.get(self.st.get(cls).owner).kind,
                SymKind::Package | SymKind::NoSymbol
            )
        };
        if !class_names.contains(name) && top_level && name != "package" {
            self.emit_forwarder(&this_name, &forwarded, cls);
        }
    }

    fn emit_module_init(&self, b: &mut ClassBuilder, class_id: SymbolId, body: &[Tree]) {
        let class_name = b.this_name.clone();
        let st = self.st;
        let inits: Vec<&Tree> = body
            .iter()
            .filter(|t| matches!(t.kind, TreeKind::ValDef { .. }))
            .collect();
        let extras = &self.extras;
        let lambda_n = &self.lambda_n;
        let source = self.source_name;
        let library_abi = self.library_abi;
        let boxed_vars = &self.boxed_vars;
        let delayed = extends_delayed_init(st, class_id);
        let is_app = extends_app(st, class_id);
        let super_name = b.super_name.clone();
        b.add_code(ACC_PRIVATE, "<init>", "()V", 4, |asm| {
            let mut frame = Frame::instance();
            asm.aload(0);
            asm.invokespecial(&super_name, "<init>", "()V");
            asm.aload(0);
            asm.putstatic(&class_name, "MODULE$", &format!("L{class_name};"));
            let ctx = emit_ctx(
                st,
                class_id,
                &class_name,
                Type::Unit,
                extras,
                lambda_n,
                source,
                library_abi,
                boxed_vars,
            );
            if delayed {
                if library_abi && is_app {
                    asm.aload(0);
                    asm.invokestatic_interface("scala/App", "$init$", "(Lscala/App;)V");
                }
                Gen::emit_delayed_init_call(asm, &class_name);
            } else {
                for vd in &inits {
                    if let TreeKind::ValDef {
                        name, mods, rhs, ..
                    } = &vd.kind
                    {
                        if rhs.is_empty() || mods.flags.contains(Flags::LAZY) {
                            continue;
                        }
                        asm.aload(0);
                        gen_expr(asm, &mut frame, &ctx, rhs);
                        let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                            st.get(vd.sym).ty.clone()
                        } else {
                            vd.ty.clone()
                        };
                        asm.putfield(&class_name, name, &jvm_desc(st, &ty));
                    }
                }
            }
            asm.vreturn();
        });
    }

    fn emit_module_clinit(&self, b: &mut ClassBuilder) {
        let class_name = b.this_name.clone();
        b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
            asm.new_obj(&class_name);
            asm.dup();
            asm.invokespecial(&class_name, "<init>", "()V");
            asm.pop();
            asm.vreturn();
        });
    }

    fn emit_case_companion(&mut self, class_tree: &Tree) {
        let class_id = class_tree.sym;
        let class_jvm = if class_id.is_none() {
            class_tree.name().unwrap_or("X").to_string()
        } else {
            class_internal(self.st, class_id)
        };
        let this_name = format!("{class_jvm}$");
        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        b.fields.push(Field {
            access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
            name: "MODULE$".into(),
            desc: format!("L{this_name};"),
        });
        self.emit_module_init(&mut b, class_id, &[]);
        self.emit_module_clinit(&mut b);
        emit_case_apply(&mut b, self.st, class_id);
        attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
        self.out.push(b.finish());
    }

    fn emit_forwarder(
        &mut self,
        module_jvm: &str,
        methods: &[(String, String, Type, Vec<Type>)],
        class_id: SymbolId,
    ) {
        let fwd_name = strip_module_dollar(module_jvm);
        let mut b = ClassBuilder::new(fwd_name, self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        let module_desc = format!("L{module_jvm};");
        for (name, desc, ret, params) in methods {
            let mut locals = 0u16;
            let mut loads = Vec::new();
            for p in params {
                let sort = jvm_sort(p);
                loads.push((locals, sort));
                locals += sort.slots();
            }
            let max_locals = locals.max(1);
            let ret = ret.clone();
            let name = name.clone();
            let desc = desc.clone();
            let module_jvm = module_jvm.to_string();
            let module_desc = module_desc.clone();
            b.add_code(ACC_PUBLIC | ACC_STATIC, &name, &desc, max_locals, |asm| {
                asm.getstatic(&module_jvm, "MODULE$", &module_desc);
                for (slot, sort) in &loads {
                    load(asm, *slot, *sort);
                }
                asm.invokevirtual(&module_jvm, &name, &desc);
                emit_return(asm, &ret);
            });
        }
        attach_scala_sig(&mut b, self.st, class_id, &self.pickles);
        self.out.push(b.finish());
    }

    fn find_class_named(&self, name: &str) -> Option<SymbolId> {
        self.st.symbols.iter().find_map(|s| {
            if s.kind == SymKind::Class && s.name == name && !s.flags.contains(Flags::TRAIT) {
                Some(s.id)
            } else {
                None
            }
        })
    }
}

fn emit_case_apply(b: &mut ClassBuilder, st: &SymbolTable, class_id: SymbolId) {
    let fields = st.get(class_id).ctor_fields.clone();
    let class_jvm = class_internal(st, class_id);
    let mut params = Vec::new();
    let mut locals = 1u16;
    let mut loads = Vec::new();
    for f in &fields {
        let ty = st.get(*f).ty.clone();
        let sort = jvm_sort(&ty);
        loads.push((locals, sort));
        locals += sort.slots();
        params.push(ty);
    }
    let ret = Type::Class {
        sym: class_id,
        args: vec![],
    };
    let desc = jvm_method_desc(st, &params, &ret);
    let ctor_d = jvm_method_desc(st, &params, &Type::Unit);
    b.add_code(ACC_PUBLIC, "apply", &desc, locals.max(1), |asm| {
        asm.new_obj(&class_jvm);
        asm.dup();
        for (slot, sort) in &loads {
            load(asm, *slot, *sort);
        }
        asm.invokespecial(&class_jvm, "<init>", &ctor_d);
        asm.areturn();
    });
}

// ---------------------------------------------------------------------------
// bytecode helpers
// ---------------------------------------------------------------------------

fn load(asm: &mut Assembler, slot: u16, sort: JvmSort) {
    match sort {
        JvmSort::Int => asm.iload(slot),
        JvmSort::Long => asm.lload(slot),
        JvmSort::Double => asm.dload(slot),
        JvmSort::Float => asm.fload(slot),
        JvmSort::Ref => asm.aload(slot),
        JvmSort::Void => {}
    }
}

fn store(asm: &mut Assembler, slot: u16, sort: JvmSort) {
    match sort {
        JvmSort::Int => asm.istore(slot),
        JvmSort::Long => asm.lstore(slot),
        JvmSort::Double => asm.dstore(slot),
        JvmSort::Float => asm.fstore(slot),
        JvmSort::Ref => asm.astore(slot),
        JvmSort::Void => {}
    }
}

fn pop_if_value(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Void => {}
        JvmSort::Long | JvmSort::Double => asm.pop2(),
        _ => asm.pop(),
    }
}

fn emit_return(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Void => asm.vreturn(),
        JvmSort::Int => asm.ireturn(),
        JvmSort::Long => asm.lreturn(),
        JvmSort::Double => asm.dreturn(),
        JvmSort::Float => asm.freturn(),
        JvmSort::Ref => asm.areturn(),
    }
}

const NLRC: &str = "scala/runtime/NonLocalReturnControl";

fn emit_nonlocal_return(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, expr: &Tree) {
    asm.new_obj(NLRC);
    asm.dup();
    load_this(asm, ctx);
    if !expr.is_empty() && !is_unit_like(&expr.ty) {
        gen_expr(asm, frame, ctx, expr);
        emit_box(asm, &expr.ty);
    } else {
        if !expr.is_empty() {
            gen_expr(asm, frame, ctx, expr);
            pop_if_value(asm, &expr.ty);
        }
        asm.aconst_null();
    }
    asm.invokespecial(NLRC, "<init>", "(Ljava/lang/Object;Ljava/lang/Object;)V");
    asm.athrow();
}

fn finish_method_body(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    rhs: &Tree,
    ret: &Type,
) {
    let wrap = !ctx.method_sym.is_none() && tree_has_nlr_to(rhs, ctx.method_sym);
    if wrap {
        asm.capture_try_locals();
        let start = asm.fresh_label();
        let end = asm.fresh_label();
        let handler = asm.fresh_label();
        asm.mark(start);
        emit_body_return(asm, frame, ctx, rhs, ret);
        asm.mark(end);
        asm.mark(handler);
        asm.enter_handler_captured_locals();
        asm.checkcast(NLRC);
        asm.dup();
        asm.invokevirtual(NLRC, "key", "()Ljava/lang/Object;");
        asm.aload(0);
        let rethrow = asm.fresh_label();
        asm.if_acmpne(rethrow);
        asm.invokevirtual(NLRC, "value", "()Ljava/lang/Object;");
        if is_unit_like(ret) {
            asm.pop();
            asm.vreturn();
        } else {
            emit_unbox(asm, ret);
            emit_return(asm, ret);
        }
        asm.mark(rethrow);
        asm.athrow();
        asm.exception(start, end, handler, Some(NLRC));
    } else {
        emit_body_return(asm, frame, ctx, rhs, ret);
    }
}

fn emit_body_return(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, rhs: &Tree, ret: &Type) {
    gen_expr(asm, frame, ctx, rhs);
    if is_unit_like(ret) {
        pop_if_value(asm, &rhs.ty);
        asm.vreturn();
    } else {
        emit_return(asm, ret);
    }
}

fn tree_contains_return(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::Return { .. } => true,
        TreeKind::Select { qual, .. } => tree_contains_return(qual),
        TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
            tree_contains_return(fun) || args.iter().any(tree_contains_return)
        }
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
            tree_contains_return(fun)
        }
        TreeKind::Block { stats, expr } => {
            stats.iter().any(tree_contains_return) || tree_contains_return(expr)
        }
        TreeKind::If { cond, thenp, elsep } => {
            tree_contains_return(cond) || tree_contains_return(thenp) || tree_contains_return(elsep)
        }
        TreeKind::Assign { lhs, rhs } => tree_contains_return(lhs) || tree_contains_return(rhs),
        TreeKind::ValDef { rhs, .. } => tree_contains_return(rhs),
        TreeKind::Function { body, vparams } => {
            vparams.iter().any(tree_contains_return) || tree_contains_return(body)
        }
        TreeKind::Match { selector, cases } => {
            tree_contains_return(selector)
                || cases.iter().any(|c| {
                    tree_contains_return(&c.pat)
                        || tree_contains_return(&c.guard)
                        || tree_contains_return(&c.body)
                })
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            tree_contains_return(block)
                || catches.iter().any(|c| tree_contains_return(&c.body))
                || tree_contains_return(finalizer)
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            tree_contains_return(cond) || tree_contains_return(body)
        }
        TreeKind::Throw { expr } => tree_contains_return(expr),
        TreeKind::InterpolatedString { args, .. } => args.iter().any(tree_contains_return),
        _ => false,
    }
}

fn tree_has_nlr_to(tree: &Tree, meth: SymbolId) -> bool {
    fn walk(t: &Tree, meth: SymbolId, in_fun: bool) -> bool {
        match &t.kind {
            TreeKind::Return { expr } => {
                (in_fun && (t.sym == meth || t.sym.is_none())) || walk(expr, meth, in_fun)
            }
            TreeKind::Function { vparams, body } => {
                vparams.iter().any(|p| walk(p, meth, in_fun)) || walk(body, meth, true)
            }
            TreeKind::DefDef { vparamss, rhs, .. } => {
                vparamss.iter().flatten().any(|p| walk(p, meth, in_fun)) || walk(rhs, meth, false)
            }
            TreeKind::Select { qual, .. } => walk(qual, meth, in_fun),
            TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
                walk(fun, meth, in_fun) || args.iter().any(|a| walk(a, meth, in_fun))
            }
            TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
                walk(fun, meth, in_fun)
            }
            TreeKind::Block { stats, expr } => {
                stats.iter().any(|s| walk(s, meth, in_fun)) || walk(expr, meth, in_fun)
            }
            TreeKind::If { cond, thenp, elsep } => {
                walk(cond, meth, in_fun) || walk(thenp, meth, in_fun) || walk(elsep, meth, in_fun)
            }
            TreeKind::Assign { lhs, rhs } => walk(lhs, meth, in_fun) || walk(rhs, meth, in_fun),
            TreeKind::ValDef { rhs, .. } => walk(rhs, meth, in_fun),
            TreeKind::Match { selector, cases } => {
                walk(selector, meth, in_fun)
                    || cases.iter().any(|c| {
                        walk(&c.pat, meth, in_fun)
                            || walk(&c.guard, meth, in_fun)
                            || walk(&c.body, meth, in_fun)
                    })
            }
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                walk(block, meth, in_fun)
                    || catches.iter().any(|c| walk(&c.body, meth, in_fun))
                    || walk(finalizer, meth, in_fun)
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                walk(cond, meth, in_fun) || walk(body, meth, in_fun)
            }
            TreeKind::Throw { expr } => walk(expr, meth, in_fun),
            TreeKind::InterpolatedString { args, .. } => args.iter().any(|a| walk(a, meth, in_fun)),
            _ => false,
        }
    }
    walk(tree, meth, false)
}

fn java_deprecated_desc(mods: &scala_rs_parser::Modifiers) -> Option<&'static str> {
    for a in &mods.annotations {
        let p = a.annotation_path();
        if matches!(p.as_str(), "Deprecated" | "java.lang.Deprecated") {
            return Some("Ljava/lang/Deprecated;");
        }
    }
    None
}

fn throw_runtime(asm: &mut Assembler, msg: &str) {
    asm.new_obj("java/lang/RuntimeException");
    asm.dup();
    asm.ldc_string(msg);
    asm.invokespecial(
        "java/lang/RuntimeException",
        "<init>",
        "(Ljava/lang/String;)V",
    );
    asm.athrow();
}

fn throw_not_implemented(asm: &mut Assembler) {
    asm.new_obj("scala/NotImplementedError");
    asm.dup();
    asm.invokespecial("scala/NotImplementedError", "<init>", "()V");
    asm.athrow();
}

fn push_default(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Void => {}
        JvmSort::Int => asm.iconst(0),
        JvmSort::Long => asm.lconst(0),
        JvmSort::Double => asm.dconst(0.0),
        JvmSort::Float => asm.fconst(0.0),
        JvmSort::Ref => asm.aconst_null(),
    }
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

fn gen_stat(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    match &tree.kind {
        TreeKind::ValDef { rhs, .. } => {
            let ty = if tree.ty.is_no_type() && !tree.sym.is_none() {
                ctx.st.get(tree.sym).ty.clone()
            } else {
                tree.ty.clone()
            };
            let sort = jvm_sort(&ty);
            if rhs.is_empty() {
                if is_boxed_var(ctx, tree.sym) {
                    push_default(asm, &ty);
                    emit_runtime_ref_create(asm, &ty);
                    let slot = frame.alloc(tree.sym, JvmSort::Ref);
                    store(asm, slot, JvmSort::Ref);
                    return;
                }
                frame.alloc(tree.sym, sort);
                return;
            }
            if sort == JvmSort::Void {
                gen_stat(asm, frame, ctx, rhs);
                frame.alloc(tree.sym, sort);
                return;
            }
            gen_expr(asm, frame, ctx, rhs);
            if is_boxed_var(ctx, tree.sym) {
                emit_runtime_ref_create(asm, &ty);
                let slot = frame.alloc(tree.sym, JvmSort::Ref);
                store(asm, slot, JvmSort::Ref);
                return;
            }
            let slot = frame.alloc(tree.sym, sort);
            store(asm, slot, sort);
        }
        TreeKind::DefDef { .. } | TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {
            // nested member: not lifted in this pass
        }
        TreeKind::Import { .. } | TreeKind::TypeDef { .. } | TreeKind::Empty => {}
        _ => {
            gen_expr(asm, frame, ctx, tree);
            pop_if_value(asm, &tree.ty);
        }
    }
}

fn gen_expr(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    match &tree.kind {
        TreeKind::Empty => {}
        TreeKind::Literal { lit } => gen_literal(asm, lit),
        TreeKind::This { qual } => {
            if let Some(name) = qual {
                load_qualified_this(asm, ctx, name);
            } else {
                load_this(asm, ctx);
            }
        }
        TreeKind::Super { .. } => load_this(asm, ctx),
        TreeKind::Ident { .. } => gen_ident(asm, frame, ctx, tree),
        TreeKind::Select { qual, name } => gen_select(asm, frame, ctx, tree, qual, name),
        TreeKind::Apply { fun, args } => gen_apply(asm, frame, ctx, tree, fun, args),
        TreeKind::TypeApply { fun, args } => {
            // `x.isInstanceOf[T]` is `instanceof`; `x.asInstanceOf[T]` is a
            // checkcast (or an unbox for a primitive target).
            let ic = if fun.sym.is_none() {
                Intrinsic::None
            } else {
                ctx.st.get(fun.sym).intrinsic
            };
            match (ic, &fun.kind) {
                (Intrinsic::IsInstanceOf, TreeKind::Select { qual, .. }) => {
                    gen_expr(asm, frame, ctx, qual);
                    if is_jvm_primitive(&qual.ty) && !is_unit_like(&qual.ty) {
                        emit_box(asm, &qual.ty);
                    }
                    let target = args.first().map(|a| a.ty.clone()).unwrap_or(Type::AnyRef);
                    asm.instanceof(&type_jvm_name(ctx.st, &target));
                }
                (Intrinsic::AsInstanceOf, TreeKind::Select { qual, .. }) => {
                    gen_expr(asm, frame, ctx, qual);
                    let target = args.first().map(|a| a.ty.clone()).unwrap_or(Type::AnyRef);
                    if is_jvm_primitive(&qual.ty) && !is_jvm_primitive(&target) {
                        emit_box(asm, &qual.ty);
                    }
                    if !is_jvm_primitive(&qual.ty) {
                        emit_from_erased_object(asm, ctx.st, &target);
                    }
                }
                _ => gen_expr(asm, frame, ctx, fun),
            }
        }
        TreeKind::Function { .. } => gen_function(asm, frame, ctx, tree),
        TreeKind::Typed { expr, .. } => {
            gen_expr(asm, frame, ctx, expr);
            // optional checkcast to a class type
            if let Type::Class { sym, .. } = &tree.ty {
                if !is_interface_sym(ctx.st, *sym) {
                    // skip; subclass assignment does not need checkcast
                }
            }
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                gen_stat(asm, frame, ctx, s);
            }
            gen_expr(asm, frame, ctx, expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            gen_if(asm, frame, ctx, cond, thenp, elsep, &tree.ty);
        }
        TreeKind::While { cond, body } => {
            let start = asm.fresh_label();
            let end = asm.fresh_label();
            asm.mark(start);
            gen_expr(asm, frame, ctx, cond);
            asm.ifeq(end);
            gen_stat(asm, frame, ctx, body);
            asm.goto(start);
            asm.mark(end);
        }
        TreeKind::DoWhile { cond, body } => {
            let start = asm.fresh_label();
            asm.mark(start);
            gen_stat(asm, frame, ctx, body);
            gen_expr(asm, frame, ctx, cond);
            asm.ifne(start);
        }
        TreeKind::Assign { lhs, rhs } => gen_assign(asm, frame, ctx, lhs, rhs),
        TreeKind::Match { selector, cases } => {
            gen_match(asm, frame, ctx, selector, cases, &tree.ty);
        }
        TreeKind::New { tpt } => gen_new(asm, frame, ctx, tpt, &[], SymbolId::NONE),
        TreeKind::Return { expr } => {
            if !ctx.method_sym.is_none() && (tree.sym == ctx.method_sym || tree.sym.is_none()) {
                if !expr.is_empty() && !is_unit_like(&expr.ty) && !is_unit_like(&ctx.ret_ty) {
                    gen_expr(asm, frame, ctx, expr);
                } else if !expr.is_empty() && !is_unit_like(&ctx.ret_ty) {
                    gen_expr(asm, frame, ctx, expr);
                } else if !expr.is_empty() {
                    gen_expr(asm, frame, ctx, expr);
                    pop_if_value(asm, &expr.ty);
                }
                emit_return(asm, &ctx.ret_ty);
                // Dead code after `return` still needs a dummy for the method
                // epilogue (and for StackMapTable at the next instruction).
                push_default(asm, &ctx.ret_ty);
            } else {
                emit_nonlocal_return(asm, frame, ctx, expr);
                push_default(asm, &ctx.ret_ty);
            }
        }
        TreeKind::Throw { expr } => {
            gen_expr(asm, frame, ctx, expr);
            asm.athrow();
            push_default(asm, &tree.ty);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            gen_try(asm, frame, ctx, block, catches, finalizer, &tree.ty);
        }
        TreeKind::InterpolatedString {
            prefix,
            parts,
            args,
        } => {
            if prefix == "f" {
                gen_f_interpolated(asm, frame, ctx, parts, args);
            } else {
                gen_interpolated(asm, frame, ctx, parts, args);
            }
        }
        TreeKind::ValDef { .. } => {
            gen_stat(asm, frame, ctx, tree);
        }
        TreeKind::Import { .. } | TreeKind::TypeDef { .. } => {}
        _ => {
            throw_runtime(
                asm,
                &format!(
                    "unimplemented expression: {}",
                    tree.name().unwrap_or("<tree>")
                ),
            );
            push_default(asm, &tree.ty);
        }
    }
}

fn gen_literal(asm: &mut Assembler, lit: &Lit) {
    match lit {
        Lit::Unit => {}
        Lit::Boolean(b) => asm.iconst(if *b { 1 } else { 0 }),
        Lit::Int(n) => asm.iconst(*n),
        Lit::Long(n) => asm.lconst(*n),
        Lit::Float(n) => asm.fconst(*n),
        Lit::Double(n) => asm.dconst(*n),
        Lit::Char(c) => asm.iconst(*c as i32),
        Lit::String(s) => asm.ldc_string(s),
        Lit::Null => asm.aconst_null(),
        Lit::Symbol(s) => asm.ldc_string(s),
    }
}

fn load_this(asm: &mut Assembler, ctx: &EmitCtx) {
    if let Some((lclass, field, desc)) = ctx.outer {
        asm.aload(0);
        asm.getfield(lclass, field, desc);
    } else {
        asm.aload(0);
    }
}

fn load_qualified_this(asm: &mut Assembler, ctx: &EmitCtx, name: &str) {
    let target = ctx
        .st
        .enclosing_class_named(ctx.class_sym, name)
        .unwrap_or(ctx.class_sym);
    load_this(asm, ctx);
    let mut cur = ctx.class_sym;
    while !cur.is_none() && cur != target {
        let Some(outer) = enclosing_instance(ctx.st, cur) else {
            break;
        };
        let owner = class_internal(ctx.st, cur);
        let od = format!("L{};", class_internal(ctx.st, outer));
        asm.getfield(&owner, "$outer", &od);
        cur = outer;
    }
}

fn gen_ident(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    if matches!(&tree.kind, TreeKind::Ident { name } if name == "$classOf") {
        gen_java_class_of(asm, ctx, &tree.ty);
        return;
    }
    let id = tree.sym;
    if id.is_none() {
        throw_runtime(
            asm,
            &format!("unresolved ident {}", tree.name().unwrap_or("?")),
        );
        push_default(asm, &tree.ty);
        return;
    }
    let ic = ctx.st.get(id).intrinsic;
    if matches!(ic, Intrinsic::NotImplemented) {
        if ctx.library_abi {
            emit_predef_nyi(asm);
            if is_unit_like(&tree.ty) || matches!(tree.ty, Type::Nothing) {
                asm.pop();
            }
        } else {
            throw_not_implemented(asm);
            push_default(asm, &tree.ty);
        }
        return;
    }
    if let Some((slot, sort)) = frame.get(id) {
        if is_boxed_var(ctx, id) {
            load(asm, slot, JvmSort::Ref);
            load_runtime_ref_elem(asm, ctx, &ctx.st.get(id).ty);
            return;
        }
        load(asm, slot, sort);
        return;
    }
    let sym = ctx.st.get(id);
    if sym.flags.contains(Flags::LAZY) && sym.kind == SymKind::Term {
        load_this(asm, ctx);
        let owner = class_internal(ctx.st, sym.owner);
        let desc = format!("(){}", jvm_desc(ctx.st, &sym.ty));
        asm.invokevirtual(&owner, &sym.name, &desc);
        return;
    }
    match sym.kind {
        SymKind::Term => {
            let owner = sym.owner;
            if sym.flags.contains(Flags::SYNTHETIC) && !sym.flags.contains(Flags::PARAM) {
                load_this(asm, ctx);
                if let Some(cls) = ctx.st.class_sym_of(&sym.ty) {
                    maybe_checkcast_owner(asm, ctx, cls);
                }
                return;
            }
            if is_module_class(ctx.st, owner)
                && module_class_id(ctx.st, owner) != module_class_id(ctx.st, ctx.class_sym)
            {
                let jvm = class_internal(ctx.st, module_class_id(ctx.st, owner));
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
            } else {
                load_owner_instance(asm, ctx, owner);
            }
            if is_trait_owned_term(ctx.st, id) {
                let owner = class_internal(ctx.st, owner);
                let desc = format!("(){}", jvm_desc(ctx.st, &sym.ty));
                asm.invokeinterface(&owner, &sym.name, &desc);
            } else {
                let owner = class_internal(ctx.st, owner);
                let desc = jvm_desc(ctx.st, &sym.ty);
                // A library constructor field is private; `jvm_name` holds the
                // accessor to call instead (`StringContext.parts`).
                if sym.jvm_name.is_empty() {
                    asm.getfield(&owner, &sym.name, &desc);
                } else {
                    let acc = sym.jvm_name.clone();
                    asm.invokevirtual(&owner, &acc, &format!("(){desc}"));
                }
            }
        }
        SymKind::Module | SymKind::ModuleClass => {
            let jvm = class_internal(ctx.st, module_class_id(ctx.st, id));
            asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
        }
        SymKind::Class => {
            // Java classes have no companion MODULE$. Scala `Foo.bar` still
            // loads `Foo$` when a companion exists.
            if ctx.st.get(id).flags.contains(Flags::JAVA) || ctx.st.companion_module(id).is_none() {
                return;
            }
            let jvm = format!("{}$", class_internal(ctx.st, id));
            asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
        }
        SymKind::Method => {
            let owner = ctx.st.get(id).owner;
            if is_module_class(ctx.st, owner) {
                let jvm = class_internal(ctx.st, module_class_id(ctx.st, owner));
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
            } else {
                load_owner_instance(asm, ctx, owner);
            }
            invoke_method(asm, ctx, id, Some(&tree.ty));
        }
        _ => {
            throw_runtime(asm, &format!("cannot load {}", sym.name));
            push_default(asm, &tree.ty);
        }
    }
}

fn gen_structural_call(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    recv: &Tree,
    name: &str,
    args: &[Tree],
    result: &Type,
) {
    // Scala 2.13 reflective call: getClass.getMethod(name, Class[]).invoke(recv, Object[])
    gen_expr(asm, frame, ctx, recv);
    asm.dup();
    asm.invokevirtual("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    asm.ldc_string(&encode_method_name(name));
    asm.iconst(args.len() as i32);
    asm.anewarray("java/lang/Class");
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_java_class_of(asm, ctx, &a.ty);
        asm.aastore();
    }
    asm.invokevirtual(
        "java/lang/Class",
        "getMethod",
        "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
    );
    asm.swap();
    asm.iconst(args.len() as i32);
    asm.anewarray("java/lang/Object");
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_expr(asm, frame, ctx, a);
        emit_box(asm, &a.ty);
        asm.aastore();
    }
    asm.invokevirtual(
        "java/lang/reflect/Method",
        "invoke",
        "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
    );
    match result {
        Type::Int
        | Type::Boolean
        | Type::Long
        | Type::Double
        | Type::Char
        | Type::Float
        | Type::Byte
        | Type::Short => {
            emit_unbox(asm, result);
        }
        Type::Unit | Type::NoType => {
            asm.pop();
        }
        Type::String => {
            asm.checkcast("java/lang/String");
        }
        _ => {
            if let Some(cn) = checkcast_internal(ctx.st, result) {
                asm.checkcast(&cn);
            }
        }
    }
}

fn gen_java_class_of(asm: &mut Assembler, ctx: &EmitCtx, ty: &Type) {
    match ty {
        Type::Int => asm.getstatic("java/lang/Integer", "TYPE", "Ljava/lang/Class;"),
        Type::Boolean => asm.getstatic("java/lang/Boolean", "TYPE", "Ljava/lang/Class;"),
        Type::Long => asm.getstatic("java/lang/Long", "TYPE", "Ljava/lang/Class;"),
        Type::Double => asm.getstatic("java/lang/Double", "TYPE", "Ljava/lang/Class;"),
        Type::Float => asm.getstatic("java/lang/Float", "TYPE", "Ljava/lang/Class;"),
        Type::Char => asm.getstatic("java/lang/Character", "TYPE", "Ljava/lang/Class;"),
        Type::Byte => asm.getstatic("java/lang/Byte", "TYPE", "Ljava/lang/Class;"),
        Type::Short => asm.getstatic("java/lang/Short", "TYPE", "Ljava/lang/Class;"),
        Type::Unit | Type::NoType => asm.getstatic("java/lang/Void", "TYPE", "Ljava/lang/Class;"),
        Type::String => asm.ldc_class("java/lang/String"),
        Type::Class { sym, .. } | Type::ModuleRef(sym) => {
            asm.ldc_class(&class_internal(ctx.st, *sym));
        }
        Type::Named { name, .. } => asm.ldc_class(&name.replace('.', "/")),
        _ => asm.ldc_class("java/lang/Object"),
    }
}

fn gen_select(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tree: &Tree,
    qual: &Tree,
    name: &str,
) {
    if name == "length" && matches!(qual.ty, Type::Array(_)) {
        gen_expr(asm, frame, ctx, qual);
        asm.arraylength();
        return;
    }
    if name == "length" && !tree.sym.is_none() && ctx.st.get(tree.sym).owner == ctx.st.array_sym {
        // Generic `Array[T]` erases to Object; nsc uses ScalaRunTime.array_length.
        if ctx.library_abi {
            asm.getstatic(
                "scala/runtime/ScalaRunTime$",
                "MODULE$",
                "Lscala/runtime/ScalaRunTime$;",
            );
            gen_expr(asm, frame, ctx, qual);
            asm.invokevirtual(
                "scala/runtime/ScalaRunTime$",
                "array_length",
                "(Ljava/lang/Object;)I",
            );
        } else {
            gen_expr(asm, frame, ctx, qual);
            asm.arraylength();
        }
        return;
    }
    if matches!(qual.ty, Type::Refined { .. }) && tree.sym.is_none() {
        gen_structural_call(asm, frame, ctx, qual, name, &[], &tree.ty);
        return;
    }
    if !tree.sym.is_none() {
        let s = ctx.st.get(tree.sym);
        if s.flags.contains(Flags::LAZY) && s.kind == SymKind::Term {
            gen_expr(asm, frame, ctx, qual);
            let owner = class_internal(ctx.st, s.owner);
            let desc = format!("(){}", jvm_desc(ctx.st, &s.ty));
            asm.invokevirtual(&owner, &s.name, &desc);
            return;
        }
        match s.kind {
            SymKind::Term => {
                if s.flags.contains(Flags::STATIC) {
                    let owner = class_internal(ctx.st, s.owner);
                    let desc = if !s.jvm_name.is_empty() && !s.jvm_name.starts_with('(') {
                        s.jvm_name.clone()
                    } else {
                        jvm_desc(ctx.st, &s.ty)
                    };
                    asm.getstatic(&owner, &s.name, &desc);
                    maybe_cast_erased_load(asm, ctx, &s.ty, &tree.ty);
                    return;
                }
                gen_expr(asm, frame, ctx, qual);
                checkcast_refined_receiver(asm, ctx, &qual.ty, tree.sym);
                if is_trait_owned_term(ctx.st, tree.sym) {
                    let owner = class_internal(ctx.st, s.owner);
                    let desc = format!("(){}", jvm_desc(ctx.st, &s.ty));
                    asm.invokeinterface(&owner, &s.name, &desc);
                } else {
                    let owner = class_internal(ctx.st, s.owner);
                    let desc = jvm_desc(ctx.st, &s.ty);
                    if s.jvm_name.is_empty() {
                        asm.getfield(&owner, &s.name, &desc);
                    } else {
                        let acc = s.jvm_name.clone();
                        asm.invokevirtual(&owner, &acc, &format!("(){desc}"));
                    }
                    maybe_cast_erased_load(asm, ctx, &s.ty, &tree.ty);
                }
                return;
            }
            SymKind::Method => {
                let ic = s.intrinsic;
                if !s.flags.contains(Flags::STATIC) {
                    gen_expr(asm, frame, ctx, qual);
                    checkcast_refined_receiver(asm, ctx, &qual.ty, tree.sym);
                }
                if matches!(qual.kind, TreeKind::Super { .. }) {
                    invoke_super(asm, ctx, tree.sym);
                } else if matches!(ic, Intrinsic::StringToInt) {
                    asm.invokestatic("java/lang/Integer", "parseInt", "(Ljava/lang/String;)I");
                } else if matches!(ic, Intrinsic::StringToLong) {
                    asm.invokestatic("java/lang/Long", "parseLong", "(Ljava/lang/String;)J");
                } else if matches!(ic, Intrinsic::StringToDouble) {
                    asm.invokestatic("java/lang/Double", "parseDouble", "(Ljava/lang/String;)D");
                } else if matches!(ic, Intrinsic::IntToLong) {
                    asm.i2l();
                } else if matches!(ic, Intrinsic::IntToDouble) {
                    asm.i2d();
                } else if matches!(ic, Intrinsic::LongToDouble) {
                    asm.l2d();
                } else if matches!(ic, Intrinsic::IntToFloat) {
                    asm.i2f();
                } else if matches!(ic, Intrinsic::LongToFloat) {
                    asm.l2f();
                } else if matches!(ic, Intrinsic::FloatToDouble) {
                    asm.f2d();
                } else if matches!(ic, Intrinsic::IntToByte) {
                    asm.i2b();
                } else if matches!(ic, Intrinsic::IntToShort) {
                    asm.i2s();
                } else if matches!(ic, Intrinsic::NotImplemented) {
                    if ctx.library_abi {
                        // Receiver was already pushed; Predef.??? is MODULE$.???().
                        asm.pop();
                        emit_predef_nyi(asm);
                        if is_unit_like(&tree.ty) || matches!(tree.ty, Type::Nothing) {
                            asm.pop();
                        }
                    } else {
                        throw_not_implemented(asm);
                        push_default(asm, &tree.ty);
                    }
                } else if ctx.st.is_value_class(ctx.st.get(tree.sym).owner) {
                    invoke_value_extension(asm, ctx, tree.sym, Some(&tree.ty));
                } else {
                    // `x.toString` on an `Int` dispatches on
                    // `java/lang/Integer` (or `java/lang/Object` for the
                    // inherited one), so box the primitive receiver.
                    let owner_jvm = class_internal(ctx.st, s.owner);
                    if !s.flags.contains(Flags::STATIC)
                        && is_jvm_primitive(&qual.ty)
                        && !is_unit_like(&qual.ty)
                        && (is_boxed_primitive(&owner_jvm) || owner_jvm == "java/lang/Object")
                    {
                        emit_box(asm, &qual.ty);
                    }
                    invoke_method(asm, ctx, tree.sym, Some(&tree.ty));
                }
                return;
            }
            SymKind::Module | SymKind::ModuleClass => {
                let jvm = class_internal(ctx.st, module_class_id(ctx.st, tree.sym));
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                return;
            }
            SymKind::Package => {
                // Package prefixes of `scala.collection.immutable.Queue` have
                // no runtime value; the module Select loads MODULE$.
                return;
            }
            _ => {}
        }
    }
    if let Some(cid) = ctx.st.class_sym_of(&qual.ty) {
        gen_expr(asm, frame, ctx, qual);
        let owner = class_internal(ctx.st, cid);
        let desc = jvm_desc(ctx.st, &tree.ty);
        asm.getfield(&owner, name, &desc);
        return;
    }
    throw_runtime(asm, &format!("select {name}"));
    push_default(asm, &tree.ty);
}

fn gen_assign(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, lhs: &Tree, rhs: &Tree) {
    match &lhs.kind {
        TreeKind::Ident { .. } => {
            let id = lhs.sym;
            if is_boxed_var(ctx, id) {
                if let Some((slot, _)) = frame.get(id) {
                    load(asm, slot, JvmSort::Ref);
                    gen_expr(asm, frame, ctx, rhs);
                    store_runtime_ref_elem(asm, &ctx.st.get(id).ty);
                    return;
                }
            }
            if let Some((slot, sort)) = frame.get(id) {
                gen_expr(asm, frame, ctx, rhs);
                store(asm, slot, sort);
                return;
            }
            if !id.is_none() {
                let s = ctx.st.get(id);
                load_this(asm, ctx);
                gen_expr(asm, frame, ctx, rhs);
                asm.putfield(
                    &class_internal(ctx.st, s.owner),
                    &s.name,
                    &jvm_desc(ctx.st, &s.ty),
                );
                return;
            }
            gen_expr(asm, frame, ctx, rhs);
            pop_if_value(asm, &rhs.ty);
        }
        TreeKind::Select { qual, name } => {
            gen_expr(asm, frame, ctx, qual);
            gen_expr(asm, frame, ctx, rhs);
            if !lhs.sym.is_none() && is_trait_owned_term(ctx.st, lhs.sym) {
                let s = ctx.st.get(lhs.sym);
                let owner = class_internal(ctx.st, s.owner);
                let desc = format!("({})V", jvm_desc(ctx.st, &s.ty));
                asm.invokeinterface(&owner, &setter_name(&s.name), &desc);
                return;
            }
            let owner = if !lhs.sym.is_none() {
                class_internal(ctx.st, ctx.st.get(lhs.sym).owner)
            } else if let Some(cid) = ctx.st.class_sym_of(&qual.ty) {
                class_internal(ctx.st, cid)
            } else {
                ctx.class_name.to_string()
            };
            let desc = if !lhs.ty.is_no_type() {
                jvm_desc(ctx.st, &lhs.ty)
            } else {
                jvm_desc(ctx.st, &rhs.ty)
            };
            asm.putfield(&owner, name, &desc);
        }
        _ => {
            gen_expr(asm, frame, ctx, rhs);
            pop_if_value(asm, &rhs.ty);
        }
    }
}

fn gen_if(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    cond: &Tree,
    thenp: &Tree,
    elsep: &Tree,
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, cond);
    let else_l = asm.fresh_label();
    let end_l = asm.fresh_label();
    asm.ifeq(else_l);
    if is_unit_like(result_ty) {
        gen_stat(asm, frame, ctx, thenp);
    } else {
        gen_expr(asm, frame, ctx, thenp);
    }
    asm.goto(end_l);
    asm.mark(else_l);
    if is_unit_like(result_ty) {
        gen_stat(asm, frame, ctx, elsep);
    } else {
        gen_expr(asm, frame, ctx, elsep);
    }
    asm.mark(end_l);
}

fn gen_new(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tpt: &Tree,
    args: &[Tree],
    ctor_sym: SymbolId,
) {
    if let Some(elem) = array_elem_ty(&tpt.ty) {
        if let Some(len) = args.first() {
            gen_expr(asm, frame, ctx, len);
            emit_newarray(asm, ctx, &elem);
            return;
        }
    }
    let class_id = tpt
        .sym
        .is_none()
        .then(|| ctx.st.class_sym_of(&tpt.ty))
        .flatten()
        .or(if tpt.sym.is_none() {
            None
        } else {
            Some(tpt.sym)
        })
        .or_else(|| ctx.st.class_sym_of(&tpt.ty))
        .unwrap_or(tpt.sym);
    let internal = if class_id.is_none() {
        tpt.name().unwrap_or("java/lang/Object").to_string()
    } else {
        class_internal(ctx.st, class_id)
    };
    let desc = if !ctor_sym.is_none() && ctx.st.get(ctor_sym).name == "<init>" {
        method_desc_from_sym(ctx.st, ctor_sym)
    } else if class_id.is_none() {
        let pts: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
        jvm_method_desc(ctx.st, &pts, &Type::Unit)
    } else {
        ctor_desc(ctx.st, class_id, args)
    };
    let desc = if class_id.is_none() {
        desc
    } else {
        desc_with_extra_params(
            &with_enclosing_outer_param(ctx.st, class_id, &desc),
            &capture_params_desc(ctx.st, ctx.boxed_vars, class_id),
        )
    };
    let field_tys: Vec<Type> = if !ctor_sym.is_none() && ctx.st.get(ctor_sym).name == "<init>" {
        match &ctx.st.get(ctor_sym).ty {
            Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
            _ => args.iter().map(|a| a.ty.clone()).collect(),
        }
    } else if class_id.is_none() {
        args.iter().map(|a| a.ty.clone()).collect()
    } else {
        let fields = ctx.st.get(class_id).ctor_fields.clone();
        if fields.is_empty() || fields.len() != args.len() {
            args.iter().map(|a| a.ty.clone()).collect()
        } else {
            fields.iter().map(|f| ctx.st.get(*f).ty.clone()).collect()
        }
    };
    asm.new_obj(&internal);
    asm.dup();
    if !class_id.is_none() {
        if enclosing_instance(ctx.st, class_id).is_some() {
            load_this(asm, ctx);
        }
    }
    for (i, a) in args.iter().enumerate() {
        gen_expr(asm, frame, ctx, a);
        let pty = field_tys.get(i).unwrap_or(&a.ty);
        if is_jvm_primitive(&a.ty) && !is_jvm_primitive(pty) {
            emit_box(asm, &a.ty);
        }
    }
    for id in class_captures(ctx.st, class_id).to_vec() {
        load_capture_arg(asm, frame, ctx, id);
    }
    asm.invokespecial(&internal, "<init>", &desc);
}

fn gen_apply(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tree: &Tree,
    fun: &Tree,
    args: &[Tree],
) {
    let fun0 = peel_fun(fun);
    let (fun, owned_args) = flatten_apply_owned(fun0, args);
    let args: &[Tree] = &owned_args;

    if let TreeKind::Select { qual, name } = &fun.kind {
        if matches!(qual.ty, Type::Refined { .. }) && fun.sym.is_none() {
            gen_structural_call(asm, frame, ctx, qual, name, args, &tree.ty);
            return;
        }
    }

    if matches!(&fun.kind, TreeKind::New { .. }) {
        let tpt = match &fun.kind {
            TreeKind::New { tpt } => tpt,
            _ => unreachable!(),
        };
        gen_new(asm, frame, ctx, tpt, args, tree.sym);
        return;
    }

    let ic = if !fun.sym.is_none() {
        ctx.st.get(fun.sym).intrinsic
    } else {
        Intrinsic::None
    };

    if ctx.library_abi && (matches!(ic, Intrinsic::Println) || fun.name() == Some("println")) {
        gen_predef_println(asm, frame, ctx, args, true);
        return;
    }
    if ctx.library_abi && (matches!(ic, Intrinsic::Print) || fun.name() == Some("print")) {
        gen_predef_println(asm, frame, ctx, args, false);
        return;
    }

    if matches!(ic, Intrinsic::Println) || fun.name() == Some("println") {
        gen_println(asm, frame, ctx, args, true);
        return;
    }
    if matches!(ic, Intrinsic::Print) || fun.name() == Some("print") {
        gen_println(asm, frame, ctx, args, false);
        return;
    }

    if fun.name() == Some("$box") {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            if is_unit_like(&a.ty) {
                // nsc boxes Unit as BoxedUnit.UNIT. ArrayOps.head already left
                // that ref (or null) on the stack; a Unit literal left nothing.
                if ctx.library_abi {
                    if !unit_leaves_boxed_ref(a, ctx.st) {
                        emit_boxed_unit(asm);
                    }
                } else {
                    emit_box(asm, &a.ty);
                }
            } else {
                emit_box(asm, &a.ty);
            }
        } else {
            asm.aconst_null();
        }
        return;
    }
    if fun.name() == Some("$unbox") {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            emit_unbox(asm, &tree.ty);
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }

    if ctx.library_abi && matches!(ic, Intrinsic::Assert) {
        gen_predef_assert_require(asm, frame, ctx, args, true);
        return;
    }
    if ctx.library_abi && matches!(ic, Intrinsic::Require) {
        gen_predef_assert_require(asm, frame, ctx, args, false);
        return;
    }
    if ctx.library_abi && matches!(ic, Intrinsic::NotImplemented) {
        emit_predef_nyi(asm);
        // `Predef.???` is declared to return `Nothing$` but always throws.
        // Drop the phantom slot so a Unit/`Nothing` statement (e.g. `try ???`)
        // does not leave a value under the catch handler.
        if is_unit_like(&tree.ty) || matches!(tree.ty, Type::Nothing) {
            asm.pop();
        }
        return;
    }

    if matches!(ic, Intrinsic::Assert) {
        gen_assert_require(asm, frame, ctx, args, true);
        return;
    }
    if matches!(ic, Intrinsic::Require) {
        gen_assert_require(asm, frame, ctx, args, false);
        return;
    }
    if matches!(ic, Intrinsic::NotImplemented) {
        throw_not_implemented(asm);
        push_default(asm, &tree.ty);
        return;
    }
    if ctx.library_abi
        && (fun.name() == Some("identity")
            || fun.name() == Some("locally")
            || fun.name() == Some("implicitly")
            || matches!(
                ic,
                Intrinsic::Identity | Intrinsic::Locally | Intrinsic::Implicitly
            ) && fun
                .name()
                .is_some_and(|n| n == "identity" || n == "locally" || n == "implicitly"))
    {
        gen_predef_poly(
            asm,
            frame,
            ctx,
            args,
            &tree.ty,
            fun.name().unwrap_or("identity"),
        );
        return;
    }

    if matches!(ic, Intrinsic::Identity) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            gen_receiver(asm, frame, ctx, fun);
        }
        return;
    }
    if matches!(ic, Intrinsic::Implicitly) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }
    if matches!(ic, Intrinsic::Locally) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            if matches!(&a.ty, Type::Function { .. }) {
                asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
                if is_unit_like(&tree.ty) {
                    asm.pop();
                } else if is_jvm_primitive(&tree.ty) {
                    emit_unbox(asm, &tree.ty);
                } else if matches!(tree.ty, Type::String) {
                    asm.checkcast("java/lang/String");
                }
            }
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }
    if matches!(ic, Intrinsic::Any2StringAdd) {
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
        } else {
            push_default(asm, &tree.ty);
        }
        return;
    }
    if matches!(ic, Intrinsic::WrapArrowAssoc) {
        // Identity: `->` is lowered to `new Tuple2` so ArrowAssoc is never allocated.
        if let Some(a) = args.first() {
            gen_expr(asm, frame, ctx, a);
            if is_jvm_primitive(&a.ty) {
                emit_box(asm, &a.ty);
            }
        } else {
            asm.aconst_null();
        }
        return;
    }
    if matches!(ic, Intrinsic::StringToInt) {
        gen_receiver(asm, frame, ctx, fun);
        asm.invokestatic("java/lang/Integer", "parseInt", "(Ljava/lang/String;)I");
        return;
    }
    if matches!(ic, Intrinsic::StringToLong) {
        gen_receiver(asm, frame, ctx, fun);
        asm.invokestatic("java/lang/Long", "parseLong", "(Ljava/lang/String;)J");
        return;
    }
    if matches!(ic, Intrinsic::StringToDouble) {
        gen_receiver(asm, frame, ctx, fun);
        asm.invokestatic("java/lang/Double", "parseDouble", "(Ljava/lang/String;)D");
        return;
    }
    if matches!(ic, Intrinsic::Eq | Intrinsic::Ne) {
        gen_eq_ne(asm, frame, ctx, fun, args, matches!(ic, Intrinsic::Eq));
        return;
    }
    if matches!(ic, Intrinsic::AnyEq | Intrinsic::AnyNe) {
        gen_any_eq(asm, frame, ctx, fun, args, matches!(ic, Intrinsic::AnyEq));
        return;
    }
    if let Intrinsic::NewTuple(n) = ic {
        // nsc lowers a tuple literal to `new TupleN`, so no `TupleN$` module
        // classfile is needed.
        let cls = format!("scala/Tuple{n}");
        asm.new_obj(&cls);
        asm.dup();
        for a in args.iter() {
            gen_expr(asm, frame, ctx, a);
            if is_jvm_primitive(&a.ty) {
                emit_box(asm, &a.ty);
            }
        }
        let desc = format!("({})V", "Ljava/lang/Object;".repeat(n));
        asm.invokespecial(&cls, "<init>", &desc);
        return;
    }
    if matches!(ic, Intrinsic::Synchronized) {
        gen_synchronized(asm, frame, ctx, fun, args, &tree.ty);
        return;
    }

    if matches!(&fun.ty, Type::Function { .. })
        || (fun.sym.is_none()
            && matches!(&tree.kind, TreeKind::Apply { .. })
            && matches!(&fun.ty, Type::Function { .. }))
    {
        gen_function_apply(asm, frame, ctx, fun, args, &tree.ty);
        return;
    }

    if !ctx.library_abi && is_arrow_assoc_arrow(ctx, fun) {
        gen_tuple2_arrow(asm, frame, ctx, fun, args);
        return;
    }

    if let TreeKind::Select { qual, name } = &fun.kind {
        match ic {
            Intrinsic::IntBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                } else {
                    asm.iconst(0);
                }
                emit_int_bin(asm, op);
                return;
            }
            Intrinsic::IntUn(op) => {
                gen_expr(asm, frame, ctx, qual);
                match op {
                    "-" => asm.ineg(),
                    "~" => {
                        asm.iconst(-1);
                        asm.ixor();
                    }
                    _ => {}
                }
                return;
            }
            Intrinsic::LongBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                widen_numeric(asm, &qual.ty, &Type::Long);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                    if !matches!(op, "<<" | ">>" | ">>>") {
                        widen_numeric(asm, &r.ty, &Type::Long);
                    }
                }
                emit_long_bin(asm, op);
                return;
            }
            Intrinsic::LongUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.lneg();
                return;
            }
            Intrinsic::DoubleBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                widen_numeric(asm, &qual.ty, &Type::Double);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                    widen_numeric(asm, &r.ty, &Type::Double);
                }
                emit_double_bin(asm, op);
                return;
            }
            Intrinsic::DoubleUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.dneg();
                return;
            }
            Intrinsic::FloatUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.fneg();
                return;
            }
            Intrinsic::BoolBin("&&") => {
                gen_bool_and(asm, frame, ctx, qual, args.first());
                return;
            }
            Intrinsic::BoolBin("||") => {
                gen_bool_or(asm, frame, ctx, qual, args.first());
                return;
            }
            Intrinsic::BoolBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                }
                emit_int_cmp(asm, op);
                return;
            }
            Intrinsic::BoolUn("!") => {
                gen_expr(asm, frame, ctx, qual);
                asm.iconst(1);
                asm.ixor();
                return;
            }
            Intrinsic::StringConcat => {
                if let Some(r) = args.first() {
                    gen_string_concat(asm, frame, ctx, qual, r);
                } else {
                    gen_expr(asm, frame, ctx, qual);
                }
                return;
            }
            Intrinsic::AnyToString => {
                gen_expr(asm, frame, ctx, qual);
                if is_jvm_primitive(&qual.ty) && !is_unit_like(&qual.ty) {
                    emit_box(asm, &qual.ty);
                }
                asm.invokevirtual("java/lang/Object", "toString", "()Ljava/lang/String;");
                return;
            }
            Intrinsic::Identity => {
                gen_expr(asm, frame, ctx, qual);
                return;
            }
            Intrinsic::IntToByte => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2b();
                return;
            }
            Intrinsic::IntToShort => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2s();
                return;
            }
            Intrinsic::IntToLong => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2l();
                return;
            }
            Intrinsic::IntToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2d();
                return;
            }
            Intrinsic::IntToFloat => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2f();
                return;
            }
            Intrinsic::LongToFloat => {
                gen_expr(asm, frame, ctx, qual);
                asm.l2f();
                return;
            }
            Intrinsic::FloatToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.f2d();
                return;
            }
            Intrinsic::LongToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.l2d();
                return;
            }
            _ => {}
        }

        if !ctx.library_abi && name == "+" && matches!(tree.ty, Type::String) {
            if let Some(r) = args.first() {
                gen_string_concat(asm, frame, ctx, qual, r);
                return;
            }
        }

        // name-based int ops if typer did not attach an intrinsic
        if args.len() == 1
            && matches!(qual.ty.widen_constant(), Type::Int)
            && matches!(args[0].ty.widen_constant(), Type::Int)
        {
            if matches!(
                name.as_str(),
                "+" | "-" | "*" | "/" | "%" | "==" | "!=" | "<" | "<=" | ">" | ">="
            ) {
                gen_expr(asm, frame, ctx, qual);
                gen_expr(asm, frame, ctx, &args[0]);
                emit_int_bin(asm, name);
                return;
            }
        }
    }

    // Function value apply (erased to FunctionN.apply)
    if matches!(&fun.ty, Type::Function { .. }) {
        gen_function_apply(asm, frame, ctx, fun, args, &tree.ty);
        return;
    }

    // regular method / apply
    if fun.sym.is_none() {
        throw_runtime(asm, "unresolved apply");
        push_default(asm, &tree.ty);
        return;
    }

    gen_receiver(asm, frame, ctx, fun);
    if let TreeKind::Select { qual, .. } = &fun.kind {
        checkcast_refined_receiver(asm, ctx, &qual.ty, fun.sym);
    }
    checkcast_erased_method_receiver(asm, ctx, fun);
    let value_owner = if !fun.sym.is_none() && ctx.st.is_value_class(ctx.st.get(fun.sym).owner) {
        Some(ctx.st.get(fun.sym).owner)
    } else {
        None
    };
    if let Some(owner) = value_owner {
        if let TreeKind::Select { qual, .. } = &fun.kind {
            box_value_class_receiver(asm, ctx, owner, qual);
        }
    }
    let param_tys: Vec<Type> = if !fun.sym.is_none() {
        match &ctx.st.get(fun.sym).ty {
            Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
            Type::Function { params, .. } => params.clone(),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let java_varargs = !fun.sym.is_none() && {
        let f = ctx.st.get(fun.sym).flags;
        f.contains(Flags::JAVA) && f.contains(Flags::VARARGS)
    };
    let array_elem_op = matches!(
        &fun.kind,
        TreeKind::Select { qual, name }
            if (name == "update" || name == "apply") && matches!(qual.ty, Type::Array(_))
    );
    gen_call_args(
        asm,
        frame,
        ctx,
        args,
        &param_tys,
        value_owner.is_some() || (ctx.library_abi && !array_elem_op),
        java_varargs,
        fun.sym,
    );
    if let TreeKind::Select { qual, name } = &fun.kind {
        if name == "apply" && matches!(qual.ty, Type::Array(_)) {
            let elem = match &qual.ty {
                Type::Array(e) => e.as_ref(),
                _ => &tree.ty,
            };
            emit_array_load(asm, elem);
            if matches!(elem, Type::String) {
                asm.checkcast("java/lang/String");
            }
            return;
        }
        if name == "update" && matches!(qual.ty, Type::Array(_)) {
            emit_array_store(asm, &qual.ty);
            return;
        }
    }
    if fun_is_super(fun) {
        invoke_super(asm, ctx, fun.sym);
    } else if value_owner.is_some() {
        invoke_value_extension(asm, ctx, fun.sym, Some(&tree.ty));
    } else {
        invoke_method(asm, ctx, fun.sym, Some(&tree.ty));
    }
}

fn fun_is_super(fun: &Tree) -> bool {
    match &peel_fun(fun).kind {
        TreeKind::Select { qual, .. } => matches!(qual.kind, TreeKind::Super { .. }),
        _ => false,
    }
}

fn invoke_super(asm: &mut Assembler, ctx: &EmitCtx, id: SymbolId) {
    let s = ctx.st.get(id);
    let desc = method_desc_from_sym(ctx.st, id);
    if is_interface_sym(ctx.st, ctx.class_sym) {
        let acc = super_accessor_name(ctx.st, ctx.class_sym, &s.name);
        let iface = class_internal(ctx.st, ctx.class_sym);
        asm.invokeinterface(&iface, &acc, &desc);
        return;
    }
    let owner_id = s.owner;
    let owner = class_internal(ctx.st, owner_id);
    if is_interface_sym(ctx.st, owner_id) {
        let static_desc = trait_static_desc(&owner, &desc);
        asm.invokestatic(&format!("{}$class", owner), &s.name, &static_desc);
    } else {
        asm.invokespecial(&owner, &s.name, &desc);
    }
}

fn invoke_value_extension(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    id: SymbolId,
    result_ty: Option<&Type>,
) {
    let s = ctx.st.get(id);
    let owner_id = s.owner;
    let owner = class_internal(ctx.st, owner_id);
    if owner == "scala/runtime/RichBoolean" && s.name == "compare" {
        // No compare$extension; nsc allocates RichBoolean and calls compare(Object).
        // stack: recv_z, arg_z
        asm.swap();
        asm.new_obj("scala/runtime/RichBoolean");
        asm.dup_x1();
        asm.swap();
        asm.invokespecial("scala/runtime/RichBoolean", "<init>", "(Z)V");
        asm.swap();
        asm.invokestatic("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;");
        asm.invokevirtual(
            "scala/runtime/RichBoolean",
            "compare",
            "(Ljava/lang/Object;)I",
        );
        return;
    }
    // `sign`/`round`/`floor`/`ceil` have no `$extension` static counterpart
    // on `RichInt`/`RichLong`/`RichDouble` (unlike `toBinaryString` etc.), so
    // nsc would allocate a real instance and call the instance method. Both
    // delegate to plain `java.lang`/`Math` statics under the hood, so call
    // those directly instead -- same result, and avoids the category-1 vs.
    // category-2 stack-shuffling that a `new`+dup-based call would need for
    // `Long`/`Double` receivers.
    if owner == "scala/runtime/RichInt" && s.name == "sign" {
        asm.invokestatic("java/lang/Integer", "signum", "(I)I");
        return;
    }
    if owner == "scala/runtime/RichLong" && s.name == "sign" {
        asm.invokestatic("java/lang/Long", "signum", "(J)I");
        asm.i2l();
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "sign" {
        asm.invokestatic("java/lang/Math", "signum", "(D)D");
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "round" {
        asm.invokestatic("java/lang/Math", "round", "(D)J");
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "floor" {
        asm.invokestatic("java/lang/Math", "floor", "(D)D");
        return;
    }
    if owner == "scala/runtime/RichDouble" && s.name == "ceil" {
        asm.invokestatic("java/lang/Math", "ceil", "(D)D");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "length" {
        // 2.13 StringOps inlines `length` to `String#length`; the jar exposes
        // `size$extension` which is the same call against the underlying String.
        asm.invokestatic(
            "scala/collection/StringOps",
            "size$extension",
            "(Ljava/lang/String;)I",
        );
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "isEmpty" {
        // Same as scalac: StringOps.isEmpty is inlined to String#isEmpty.
        asm.invokevirtual("java/lang/String", "isEmpty", "()Z");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "toUpperCase" {
        // 2.13 StringOps inlines toUpperCase/toLowerCase to String.
        asm.invokevirtual("java/lang/String", "toUpperCase", "()Ljava/lang/String;");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "toLowerCase" {
        asm.invokevirtual("java/lang/String", "toLowerCase", "()Ljava/lang/String;");
        return;
    }
    if owner == "scala/collection/StringOps" && s.name == "toArray" {
        asm.invokestatic(
            "scala/collection/StringOps",
            "toArray$extension",
            "(Ljava/lang/String;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
        );
        maybe_unbox_erased_result(
            asm,
            ctx,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            result_ty,
        );
        return;
    }
    if owner == "scala/runtime/RichInt" && s.name == "to" {
        // 2.13 `to` returns Range$Inclusive, not the abstract Range.
        asm.invokestatic(
            "scala/runtime/RichInt",
            "to$extension",
            "(II)Lscala/collection/immutable/Range$Inclusive;",
        );
        return;
    }
    if owner == "scala/runtime/RichInt" && s.name == "until" {
        asm.invokestatic(
            "scala/runtime/RichInt",
            "until$extension",
            "(II)Lscala/collection/immutable/Range;",
        );
        return;
    }
    if owner == "scala/runtime/RichByte" && (s.name == "to" || s.name == "until") {
        // RichByte has no to$extension; IntegralProxy default builds a real NumericRange.
        emit_integral_numeric_range(asm, &Type::Byte, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichShort" && (s.name == "to" || s.name == "until") {
        emit_integral_numeric_range(asm, &Type::Short, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichLong" && (s.name == "to" || s.name == "until") {
        emit_long_numeric_range(asm, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichChar" && (s.name == "to" || s.name == "until") {
        emit_integral_numeric_range(asm, &Type::Char, s.name == "to");
        return;
    }
    if owner == "scala/runtime/RichChar" && s.name == "toInt" {
        // RichChar.toInt is inlined; the jar exposes intValue$extension.
        asm.invokestatic("scala/runtime/RichChar", "intValue$extension", "(C)I");
        return;
    }
    if owner == "scala/collection/ArrayOps" {
        // 2.13 ArrayOps is AnyVal over erased Array.
        if s.name == "foreach" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "foreach$extension",
                "(Ljava/lang/Object;Lscala/Function1;)V",
            );
            return;
        }
        if s.name == "map" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "map$extension",
                "(Ljava/lang/Object;Lscala/Function1;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "filter" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "filter$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "slice" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "slice$extension",
                "(Ljava/lang/Object;II)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "flatMap" {
            let n = match &s.ty {
                Type::Method { paramss, .. } => paramss.iter().flatten().count(),
                _ => s.params.len(),
            };
            let desc = if n >= 3 {
                "(Ljava/lang/Object;Lscala/Function1;Lscala/Function1;Lscala/reflect/ClassTag;)Ljava/lang/Object;"
            } else {
                "(Ljava/lang/Object;Lscala/Function1;Lscala/reflect/ClassTag;)Ljava/lang/Object;"
            };
            asm.invokestatic("scala/collection/ArrayOps", "flatMap$extension", desc);
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "take" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "take$extension",
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "collect" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "collect$extension",
                "(Ljava/lang/Object;Lscala/PartialFunction;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "zip" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "zip$extension",
                "(Ljava/lang/Object;Lscala/collection/IterableOnce;)[Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "drop" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "drop$extension",
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "dropWhile" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "dropWhile$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "exists" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "exists$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Z",
            );
            return;
        }
        if s.name == "foldLeft" || s.name == "fold" || s.name == "foldRight" {
            let desc = "(Ljava/lang/Object;Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;";
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                desc,
            );
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "count" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "count$extension",
                "(Ljava/lang/Object;Lscala/Function1;)I",
            );
            return;
        }
        if s.name == "forall" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "forall$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Z",
            );
            return;
        }
        if s.name == "scanLeft" {
            let desc = "(Ljava/lang/Object;Ljava/lang/Object;Lscala/Function2;Lscala/reflect/ClassTag;)Ljava/lang/Object;";
            asm.invokestatic("scala/collection/ArrayOps", "scanLeft$extension", desc);
            maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            return;
        }
        if s.name == "size" || s.name == "length" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "size$extension",
                "(Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "isEmpty" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "isEmpty$extension",
                "(Ljava/lang/Object;)Z",
            );
            return;
        }
        if s.name == "nonEmpty" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "nonEmpty$extension",
                "(Ljava/lang/Object;)Z",
            );
            return;
        }
        if s.name == "find" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "find$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Lscala/Option;",
            );
            return;
        }
        if s.name == "contains" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "contains$extension",
                "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            );
            return;
        }
        if s.name == "takeRight" || s.name == "dropRight" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;I)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "takeWhile" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "takeWhile$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "indices" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "indices$extension",
                "(Ljava/lang/Object;)Lscala/collection/immutable/Range;",
            );
            return;
        }
        if s.name == "lengthCompare" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "lengthCompare$extension",
                "(Ljava/lang/Object;I)I",
            );
            return;
        }
        if s.name == "filterNot" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "filterNot$extension",
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
            );
            maybe_unbox_erased_result(
                asm,
                ctx,
                "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
                result_ty,
            );
            return;
        }
        if s.name == "headOption" || s.name == "lastOption" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;)Lscala/Option;",
            );
            return;
        }
        if s.name == "partition" || s.name == "span" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;Lscala/Function1;)Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "splitAt" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "splitAt$extension",
                "(Ljava/lang/Object;I)Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "zipWithIndex" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "zipWithIndex$extension",
                "(Ljava/lang/Object;)[Lscala/Tuple2;",
            );
            return;
        }
        if s.name == "knownSize" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "knownSize$extension",
                "(Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "sizeCompare" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "sizeCompare$extension",
                "(Ljava/lang/Object;I)I",
            );
            return;
        }
        if s.name == "lengthIs" || s.name == "sizeIs" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                &format!("{}$extension", s.name),
                "(Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "indexOf" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "indexOf$extension",
                "(Ljava/lang/Object;Ljava/lang/Object;I)I",
            );
            return;
        }
        if s.name == "copyToArray" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "copyToArray$extension",
                "(Ljava/lang/Object;Ljava/lang/Object;)I",
            );
            return;
        }
        if s.name == "iterator" {
            asm.invokestatic(
                "scala/collection/ArrayOps",
                "iterator$extension",
                "(Ljava/lang/Object;)Lscala/collection/Iterator;",
            );
            return;
        }
        asm.invokestatic(
            "scala/collection/ArrayOps",
            &format!("{}$extension", s.name),
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        );
        maybe_unbox_erased_result(
            asm,
            ctx,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            result_ty,
        );
        return;
    }
    let desc = value_extension_desc(ctx.st, id);
    if owner.contains('$') {
        // Nested Predef AnyVal: `$extension` is an instance method on the
        // companion `MODULE$`, not a static on the value class.
        let ext_owner = format!("{owner}$");
        let n_args = count_value_ext_args(&desc);
        asm.getstatic(&ext_owner, "MODULE$", &format!("L{ext_owner};"));
        if n_args == 0 {
            asm.swap();
        } else {
            asm.dup_x2();
            asm.pop();
        }
        asm.invokevirtual(&ext_owner, &format!("{}$extension", s.name), &desc);
        return;
    }
    asm.invokestatic(&owner, &format!("{}$extension", s.name), &desc);
    maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
}

fn count_value_ext_args(desc: &str) -> usize {
    // Descriptor includes the extension receiver as the first argument.
    let inner = desc
        .split_once(')')
        .map(|(a, _)| a.trim_start_matches('('))
        .unwrap_or("");
    let mut n: usize = 0;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        n += 1;
        match c {
            'L' => while chars.next().is_some_and(|ch| ch != ';') {},
            '[' => {
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                if chars.peek() == Some(&'L') {
                    chars.next();
                    while chars.next().is_some_and(|ch| ch != ';') {}
                } else {
                    chars.next();
                }
            }
            _ => {}
        }
    }
    n.saturating_sub(1)
}

fn box_value_class_receiver(asm: &mut Assembler, ctx: &EmitCtx, owner: SymbolId, qual: &Tree) {
    let under = ctx.st.value_class_underlying(owner).unwrap_or(Type::Any);
    if is_jvm_primitive(&under) {
        return;
    }
    let src = peel_identity_arg(ctx, qual);
    if is_jvm_primitive(&src.ty) {
        emit_box(asm, &src.ty);
    }
}

fn peel_identity_arg<'a>(ctx: &EmitCtx, tree: &'a Tree) -> &'a Tree {
    if let TreeKind::Apply { fun, args } = &tree.kind {
        let ic = if !fun.sym.is_none() {
            ctx.st.get(fun.sym).intrinsic
        } else {
            Intrinsic::None
        };
        if matches!(
            ic,
            Intrinsic::Identity | Intrinsic::Any2StringAdd | Intrinsic::WrapArrowAssoc
        ) {
            if let Some(a) = args.first() {
                return a;
            }
        }
    }
    tree
}

fn value_extension_desc(st: &SymbolTable, id: SymbolId) -> String {
    let owner = st.get(id).owner;
    let under = st.value_class_underlying(owner).unwrap_or(Type::Int);
    let inst = method_desc_from_sym(st, id);
    let rest = inst.strip_prefix('(').unwrap_or(&inst);
    format!("({}{}", jvm_desc(st, &under), rest)
}

fn gen_receiver(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, fun: &Tree) {
    if !fun.sym.is_none() && ctx.st.get(fun.sym).flags.contains(Flags::STATIC) {
        return;
    }
    match &fun.kind {
        TreeKind::Select { qual, .. } => {
            if !fun.sym.is_none() {
                let s = ctx.st.get(fun.sym);
                if matches!(s.kind, SymKind::Module | SymKind::ModuleClass) {
                    let jvm = class_internal(ctx.st, module_class_id(ctx.st, fun.sym));
                    asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                    return;
                }
                if s.kind == SymKind::Method && is_module_class(ctx.st, s.owner) {
                    let jvm = class_internal(ctx.st, module_class_id(ctx.st, s.owner));
                    asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                    return;
                }
                if s.kind == SymKind::Package {
                    return;
                }
            }
            gen_expr(asm, frame, ctx, qual);
            // `x.toString` on an `Int` calls `java/lang/Integer.toString`, so
            // the primitive receiver has to be boxed first.
            if is_jvm_primitive(&qual.ty) && !is_unit_like(&qual.ty) && !fun.sym.is_none() {
                let s = ctx.st.get(fun.sym);
                let owner_jvm = class_internal(ctx.st, s.owner);
                if matches!(s.intrinsic, Intrinsic::None)
                    && !ctx.st.is_value_class(s.owner)
                    && (is_boxed_primitive(&owner_jvm) || owner_jvm == "java/lang/Object")
                {
                    emit_box(asm, &qual.ty);
                }
            }
        }
        _ => {
            if fun.sym.is_none() {
                load_this(asm, ctx);
                return;
            }
            let s = ctx.st.get(fun.sym);
            if matches!(s.kind, SymKind::Module | SymKind::ModuleClass) {
                let jvm = class_internal(ctx.st, module_class_id(ctx.st, fun.sym));
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                return;
            }
            let owner = s.owner;
            if is_module_class(ctx.st, owner) {
                let jvm = class_internal(ctx.st, module_class_id(ctx.st, owner));
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
            } else if owner == ctx.class_sym || owner.is_none() {
                load_this(asm, ctx);
            } else {
                load_this(asm, ctx);
                maybe_checkcast_owner(asm, ctx, owner);
            }
        }
    }
}

fn invoke_method(asm: &mut Assembler, ctx: &EmitCtx, id: SymbolId, result_ty: Option<&Type>) {
    let s = ctx.st.get(id);
    let owner_id = s.owner;
    let mut owner = class_internal(ctx.st, owner_id);
    if owner == "scala/math" {
        // `scala.math.{abs,max,min,pow,...}` are methods directly attached to
        // the `scala/math` *package* symbol (so `scala.math.abs` resolves at
        // all -- packages otherwise have no runtime value). The real ABI
        // lives on the static-forwarder class `scala.math.package`.
        owner = "scala/math/package".to_string();
    }
    let name = s.name.as_str();
    let mut desc = method_desc_boxed(ctx.st, id, ctx.boxed_vars);
    if name == "<init>" {
        asm.invokespecial(&owner, "<init>", &desc);
        return;
    }
    if s.flags.contains(Flags::STATIC) {
        asm.invokestatic(&owner, name, &desc);
        maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
        return;
    }
    if ctx.library_abi {
        if owner == "scala/reflect/ClassTag$" {
            let desc = match name {
                "Byte" => "()Lscala/reflect/ManifestFactory$ByteManifest;",
                "Short" => "()Lscala/reflect/ManifestFactory$ShortManifest;",
                "Char" => "()Lscala/reflect/ManifestFactory$CharManifest;",
                "Int" => "()Lscala/reflect/ManifestFactory$IntManifest;",
                "Long" => "()Lscala/reflect/ManifestFactory$LongManifest;",
                "Float" => "()Lscala/reflect/ManifestFactory$FloatManifest;",
                "Double" => "()Lscala/reflect/ManifestFactory$DoubleManifest;",
                "Boolean" => "()Lscala/reflect/ManifestFactory$BooleanManifest;",
                "Unit" => "()Lscala/reflect/ManifestFactory$UnitManifest;",
                "Any" | "AnyRef" | "AnyVal" | "Object" | "Nothing" | "Null" => {
                    "()Lscala/reflect/ClassTag;"
                }
                _ => desc.as_str(),
            };
            asm.invokevirtual(&owner, name, desc);
            return;
        }
        if name == "newArray" && owner == "scala/reflect/ClassTag" {
            asm.invokeinterface(&owner, "newArray", "(I)Ljava/lang/Object;");
            if let Some(ty) = result_ty {
                if let Type::Array(elem) = ty {
                    if is_concrete_array_elem(elem) {
                        asm.checkcast(&jvm_desc(ctx.st, ty));
                    }
                }
            }
            return;
        }
        if owner == "scala/Array$" && name == "apply" {
            asm.invokevirtual(
                "scala/Array$",
                "apply",
                "(Lscala/collection/immutable/Seq;Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            );
            if let Some(ty) = result_ty {
                if let Type::Array(elem) = ty {
                    if is_concrete_array_elem(elem) {
                        asm.checkcast(&jvm_desc(ctx.st, ty));
                    }
                }
            }
            return;
        }
        if owner == "scala/util/matching/Regex" {
            match name {
                "findFirstIn" => {
                    asm.invokevirtual(
                        "scala/util/matching/Regex",
                        "findFirstIn",
                        "(Ljava/lang/CharSequence;)Lscala/Option;",
                    );
                    return;
                }
                "matches" => {
                    asm.invokevirtual(
                        "scala/util/matching/Regex",
                        "matches",
                        "(Ljava/lang/CharSequence;)Z",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_bitset(&owner) {
            match name {
                "contains" => {
                    asm.invokevirtual("scala/collection/immutable/BitSet", "contains", "(I)Z");
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/BitSet",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_list(&owner) && emit_list_core_member(asm, ctx, name, id, result_ty) {
            return;
        }
        if owner == "scala/collection/Iterator" && name == "toList" {
            // `Iterator.toList` は `IterableOnceOps` の default メソッド。
            asm.invokeinterface(
                "scala/collection/IterableOnceOps",
                "toList",
                "()Lscala/collection/immutable/List;",
            );
            return;
        }
        if name == "view" && is_stdlib_list(&owner) {
            asm.invokeinterface(
                "scala/collection/SeqOps",
                "view",
                "()Lscala/collection/SeqView;",
            );
            return;
        }
        if name == "withFilter" && is_stdlib_list(&owner) {
            asm.invokeinterface(
                "scala/collection/IterableOps",
                "withFilter",
                "(Lscala/Function1;)Lscala/collection/WithFilter;",
            );
            return;
        }
        if name == "collect" && is_stdlib_list(&owner) {
            asm.invokevirtual(
                "scala/collection/immutable/List",
                "collect",
                "(Lscala/PartialFunction;)Lscala/collection/immutable/List;",
            );
            return;
        }
        if name == "withFilter" && is_stdlib_option(&owner) {
            asm.invokevirtual(
                "scala/Option",
                "withFilter",
                "(Lscala/Function1;)Lscala/Option$WithFilter;",
            );
            return;
        }
        // nsc: `def flatten[B](implicit ev: A <:< Option[B]): Option[B]`. The
        // evidence is erased, so `<:<.refl` (an `=:=`, hence a `<:<`) is the
        // witness scalac itself would summon.
        if name == "flatten" && is_stdlib_option(&owner) {
            asm.getstatic(
                "scala/$less$colon$less$",
                "MODULE$",
                "Lscala/$less$colon$less$;",
            );
            asm.invokevirtual("scala/$less$colon$less$", "refl", "()Lscala/$eq$colon$eq;");
            asm.invokevirtual(
                "scala/Option",
                "flatten",
                "(Lscala/$less$colon$less;)Lscala/Option;",
            );
            return;
        }
        if owner == "scala/collection/WithFilter" && (name == "map" || name == "flatMap") {
            asm.invokevirtual(&owner, name, "(Lscala/Function1;)Ljava/lang/Object;");
            if let Some(ty) = result_ty {
                let cls = jvm_desc(ctx.st, ty);
                if let Some(inner) = cls.strip_prefix('L').and_then(|s| s.strip_suffix(';')) {
                    asm.checkcast(inner);
                }
            }
            return;
        }
        if name == "tail" && is_stdlib_list(&owner) {
            if is_interface_sym(ctx.st, owner_id) {
                asm.invokeinterface(&owner, "tail", "()Lscala/collection/LinearSeq;");
            } else {
                asm.invokevirtual(&owner, "tail", "()Lscala/collection/LinearSeq;");
            }
            asm.checkcast("scala/collection/immutable/List");
            return;
        } else if name == "unapplySeq" && is_list_module_owner(&owner) {
            desc = "(Lscala/collection/SeqOps;)Lscala/collection/SeqOps;".into();
        }
        if is_stdlib_map_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Map$",
                        "empty",
                        "()Lscala/collection/immutable/Map;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Map$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Map");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_vector_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector$",
                        "empty",
                        "()Lscala/collection/immutable/Vector;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Vector");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_indexedseq_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/IndexedSeq$",
                        "empty",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/IndexedSeq$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_queue_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue$",
                        "empty",
                        "()Lscala/collection/immutable/Queue;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Queue");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraybuffer_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer$",
                        "empty",
                        "()Lscala/collection/mutable/ArrayBuffer;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/ArrayBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_map_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Map$",
                        "empty",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Map");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Map$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Map");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_set_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Set$",
                        "empty",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Set");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/Set$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/Set");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraydeque_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque$",
                        "empty",
                        "()Lscala/collection/mutable/ArrayDeque;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/ArrayDeque");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_listbuffer_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer$",
                        "empty",
                        "()Lscala/collection/mutable/ListBuffer;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/ListBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashmap_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap$",
                        "empty",
                        "()Lscala/collection/mutable/HashMap;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/HashMap");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashmap_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap$",
                        "empty",
                        "()Lscala/collection/mutable/LinkedHashMap;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/LinkedHashMap");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashset_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet$",
                        "empty",
                        "()Lscala/collection/mutable/HashSet;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/HashSet");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashset_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet$",
                        "empty",
                        "()Lscala/collection/mutable/LinkedHashSet;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/LinkedHashSet");
                    return;
                }
                _ => {}
            }
        }
        if is_list_module_owner(&owner) && name == "apply" {
            asm.invokevirtual(
                "scala/collection/immutable/List$",
                "apply",
                "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
            );
            asm.checkcast("scala/collection/immutable/List");
            return;
        }
        if is_stdlib_sortedset_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/SortedSet$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Ljava/lang/Object;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/SortedSet");
                return;
            }
        }
        if is_stdlib_treeset_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/TreeSet$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Ljava/lang/Object;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/TreeSet");
                return;
            }
        }
        if is_stdlib_sortedmap_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/SortedMap$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Lscala/math/Ordering;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/SortedMap");
                return;
            }
        }
        if is_stdlib_treemap_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/TreeMap$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;Lscala/math/Ordering;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/TreeMap");
                return;
            }
        }
        if is_stdlib_bitset_module(&owner) {
            if name == "apply" {
                asm.invokevirtual(
                    "scala/collection/immutable/BitSet$",
                    "apply",
                    "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                );
                asm.checkcast("scala/collection/immutable/BitSet");
                return;
            }
        }
        if is_stdlib_set_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Set$",
                        "empty",
                        "()Lscala/collection/immutable/Set;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Set$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Set");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_set(&owner) {
            match name {
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/SetOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "+" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SetOps",
                        "+",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/SetOps;",
                    );
                    asm.checkcast("scala/collection/immutable/Set");
                    return;
                }
                "-" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SetOps",
                        "-",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/SetOps;",
                    );
                    asm.checkcast("scala/collection/immutable/Set");
                    return;
                }
                "++" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "++",
                        "(Lscala/collection/IterableOnce;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Set");
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Set");
                    return;
                }
                "map" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "map",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Set");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "head" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "head",
                        "()Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else {
                            checkcast_to(asm, ctx, result_ty, "java/lang/Object");
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_seq_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Seq$",
                        "empty",
                        "()Lscala/collection/SeqOps;",
                    );
                    asm.checkcast("scala/collection/immutable/Seq");
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Seq$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Seq");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_lazylist_module(&owner) {
            match name {
                "empty" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList$",
                        "empty",
                        "()Lscala/collection/immutable/LazyList;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList$",
                        "apply",
                        "(Lscala/collection/immutable/Seq;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/LazyList");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_seq(&owner) {
            match name {
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        } else if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_lazylist(&owner) {
            match name {
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/LazyList",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_either(&owner) {
            match name {
                "isLeft" => {
                    asm.invokevirtual("scala/util/Either", "isLeft", "()Z");
                    return;
                }
                "getOrElse" => {
                    asm.invokevirtual(
                        "scala/util/Either",
                        "getOrElse",
                        "(Lscala/Function0;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        }
                    }
                    return;
                }
                "map" => {
                    asm.invokevirtual(
                        "scala/util/Either",
                        "map",
                        "(Lscala/Function1;)Lscala/util/Either;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_either_module(&owner) && name == "apply" {
            let cls = if owner.ends_with("Left$") {
                "scala/util/Left"
            } else {
                "scala/util/Right"
            };
            asm.invokevirtual(&owner, "apply", &format!("(Ljava/lang/Object;)L{cls};"));
            return;
        }
        if is_stdlib_try(&owner) {
            match name {
                "getOrElse" => {
                    asm.invokevirtual(
                        "scala/util/Try",
                        "getOrElse",
                        "(Lscala/Function0;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        }
                    }
                    return;
                }
                "map" => {
                    asm.invokevirtual(
                        "scala/util/Try",
                        "map",
                        "(Lscala/Function1;)Lscala/util/Try;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_breaks(&owner) {
            match name {
                "breakable" => {
                    asm.invokevirtual(&owner, "breakable", "(Lscala/Function0;)V");
                    return;
                }
                "break" => {
                    asm.invokevirtual(&owner, "break", "()Lscala/runtime/Nothing$;");
                    // nsc 2.13.16: `break()` returns Nothing$ and is followed by athrow.
                    asm.athrow();
                    return;
                }
                "tryBreakable" => {
                    asm.invokevirtual(
                        &owner,
                        "tryBreakable",
                        "(Lscala/Function0;)Lscala/util/control/Breaks$TryBlock;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if owner == "scala/util/control/Breaks$TryBlock" && name == "catchBreak" {
            asm.invokeinterface(
                "scala/util/control/Breaks$TryBlock",
                "catchBreak",
                "(Lscala/Function0;)Ljava/lang/Object;",
            );
            if let Some(ty) = result_ty {
                if is_unit_like(ty) {
                    // JVM returns boxed Unit; a statement must pop it.
                    asm.pop();
                } else {
                    maybe_unbox_erased_result(
                        asm,
                        ctx,
                        "(Lscala/Function0;)Ljava/lang/Object;",
                        result_ty,
                    );
                }
            }
            return;
        }
        if owner == "scala/util/Using$" && name == "resource" {
            let desc = "(Ljava/lang/Object;Lscala/Function1;Lscala/util/Using$Releasable;)Ljava/lang/Object;";
            asm.invokevirtual("scala/util/Using$", "resource", desc);
            if result_ty.is_some_and(is_unit_like) {
                asm.pop();
            } else {
                maybe_unbox_erased_result(asm, ctx, desc, result_ty);
            }
            return;
        }
        if owner == "scala/util/Using$" && name == "apply" {
            asm.invokevirtual(
                "scala/util/Using$",
                "apply",
                "(Lscala/Function0;Lscala/Function1;Lscala/util/Using$Releasable;)Lscala/util/Try;",
            );
            return;
        }
        if owner == "scala/util/Using$" && name == "resources" {
            let desc = if s.jvm_name.starts_with('(') {
                s.jvm_name.clone()
            } else {
                "(Ljava/lang/Object;Lscala/Function0;Lscala/Function2;Lscala/util/Using$Releasable;Lscala/util/Using$Releasable;)Ljava/lang/Object;".into()
            };
            asm.invokevirtual("scala/util/Using$", "resources", &desc);
            if result_ty.is_some_and(is_unit_like) {
                asm.pop();
            } else {
                maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
            }
            return;
        }
        if owner == "scala/util/Using$Manager$" && name == "apply" {
            asm.invokevirtual(
                "scala/util/Using$Manager$",
                "apply",
                "(Lscala/Function1;)Lscala/util/Try;",
            );
            return;
        }
        if owner == "scala/util/Using$Manager" {
            match name {
                "apply" => {
                    let desc =
                        "(Ljava/lang/Object;Lscala/util/Using$Releasable;)Ljava/lang/Object;";
                    asm.invokevirtual("scala/util/Using$Manager", "apply", desc);
                    if result_ty.is_some_and(is_unit_like) {
                        asm.pop();
                    } else {
                        maybe_unbox_erased_result(asm, ctx, desc, result_ty);
                    }
                    return;
                }
                "acquire" => {
                    asm.invokevirtual(
                        "scala/util/Using$Manager",
                        "acquire",
                        "(Ljava/lang/Object;Lscala/util/Using$Releasable;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_try_module(&owner) && name == "apply" {
            match owner.as_str() {
                "scala/util/Try$" => {
                    asm.invokevirtual(
                        "scala/util/Try$",
                        "apply",
                        "(Lscala/Function0;)Lscala/util/Try;",
                    );
                    return;
                }
                "scala/util/Success$" => {
                    asm.invokevirtual(
                        "scala/util/Success$",
                        "apply",
                        "(Ljava/lang/Object;)Lscala/util/Success;",
                    );
                    return;
                }
                "scala/util/Failure$" => {
                    asm.invokevirtual(
                        "scala/util/Failure$",
                        "apply",
                        "(Ljava/lang/Throwable;)Lscala/util/Failure;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_sortedmap(&owner) {
            match name {
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SortedMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SortedMap",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/SortedMap",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_treemap(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/TreeMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/TreeMap",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/TreeMap",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_map(&owner) {
            match name {
                "updated" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "updated",
                        "(Ljava/lang/Object;Ljava/lang/Object;)Lscala/collection/immutable/MapOps;",
                    );
                    asm.checkcast("scala/collection/immutable/Map");
                    return;
                }
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "+" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "$plus",
                        "(Lscala/Tuple2;)Lscala/collection/immutable/MapOps;",
                    );
                    asm.checkcast("scala/collection/immutable/Map");
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "getOrElse" => {
                    let d = "(Ljava/lang/Object;Lscala/Function0;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/MapOps", "getOrElse", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "keys" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "keys",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "values" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "values",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "keySet" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "keySet",
                        "()Lscala/collection/immutable/Set;",
                    );
                    return;
                }
                "-" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/MapOps",
                        "-",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/MapOps;",
                    );
                    asm.checkcast("scala/collection/immutable/Map");
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Map");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "head" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "head",
                        "()Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/Tuple2");
                    return;
                }
                "foldLeft" => {
                    let d = "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/IterableOnceOps", "foldLeft", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                "withDefaultValue" => {
                    asm.invokeinterface(
                        "scala/collection/immutable/Map",
                        "withDefaultValue",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/Map;",
                    );
                    return;
                }
                "view" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "view",
                        "()Lscala/collection/MapView;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_vector(&owner) {
            match name {
                "apply" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                ":+" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "$colon$plus",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/Vector");
                    return;
                }
                "updated" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector",
                        "updated",
                        "(ILjava/lang/Object;)Lscala/collection/immutable/Vector;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Vector",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "map" | "filter" => {
                    let desc = "(Lscala/Function1;)Ljava/lang/Object;";
                    asm.invokevirtual("scala/collection/immutable/Vector", name, desc);
                    checkcast_to(asm, ctx, result_ty, "scala/collection/immutable/Vector");
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "head" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "head",
                        "()Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else {
                            checkcast_to(asm, ctx, result_ty, "java/lang/Object");
                        }
                    }
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "foldLeft" => {
                    let d = "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/IterableOnceOps", "foldLeft", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_indexedseq(&owner) {
            if name == "apply" {
                asm.invokeinterface("scala/collection/SeqOps", "apply", "(I)Ljava/lang/Object;");
                if let Some(ty) = result_ty {
                    if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                        let cls = jvm_desc(ctx.st, ty);
                        if let Some(inner) = cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                        {
                            if inner != "java/lang/Object" {
                                asm.checkcast(inner);
                            }
                        }
                    }
                }
                return;
            }
        }
        if is_stdlib_queue(&owner) {
            match name {
                "enqueue" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue",
                        "enqueue",
                        "(Ljava/lang/Object;)Lscala/collection/immutable/Queue;",
                    );
                    return;
                }
                "dequeue" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue",
                        "dequeue",
                        "()Lscala/Tuple2;",
                    );
                    return;
                }
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Queue",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if !is_jvm_primitive(ty) && !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraybuffer(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "update" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer",
                        "update",
                        "(ILjava/lang/Object;)V",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayBuffer",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "map" | "filter" | "reverse" => {
                    let d = "(Lscala/Function1;)Ljava/lang/Object;";
                    let d0 = "()Ljava/lang/Object;";
                    let desc = if name == "reverse" { d0 } else { d };
                    asm.invokevirtual("scala/collection/mutable/ArrayBuffer", name, desc);
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "append" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Buffer",
                        "append",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Buffer;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "++=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "++=",
                        "(Lscala/collection/IterableOnce;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "sortBy" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sortBy",
                        "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                "sorted" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sorted",
                        "(Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ArrayBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_arraydeque(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "prepend" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ArrayDeque",
                        "prepend",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/ArrayDeque;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_listbuffer(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer",
                        "apply",
                        "(I)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/ListBuffer",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "map" | "filter" => {
                    let desc = "(Lscala/Function1;)Ljava/lang/Object;";
                    asm.invokevirtual("scala/collection/mutable/ListBuffer", name, desc);
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "reverse" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "reverse",
                        "()Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "append" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Buffer",
                        "append",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Buffer;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "++=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "++=",
                        "(Lscala/collection/IterableOnce;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "sortBy" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sortBy",
                        "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                "sorted" => {
                    asm.invokeinterface(
                        "scala/collection/SeqOps",
                        "sorted",
                        "(Lscala/math/Ordering;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/ListBuffer");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_map(&owner) {
            let d_obj_obj = "(Ljava/lang/Object;)Ljava/lang/Object;";
            match name {
                "apply" => {
                    asm.invokeinterface("scala/collection/MapOps", "apply", d_obj_obj);
                    maybe_unbox_erased_result(asm, ctx, d_obj_obj, result_ty);
                    return;
                }
                "get" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "update" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/MapOps",
                        "update",
                        "(Ljava/lang/Object;Ljava/lang/Object;)V",
                    );
                    return;
                }
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "keys" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "keys",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "values" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "values",
                        "()Lscala/collection/Iterable;",
                    );
                    return;
                }
                "+=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Map");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Map");
                    return;
                }
                "remove" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/MapOps",
                        "remove",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "clear" => {
                    asm.invokeinterface("scala/collection/mutable/MapOps", "clear", "()V");
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Map");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "getOrElse" => {
                    let d = "(Ljava/lang/Object;Lscala/Function0;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/MapOps", "getOrElse", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                "getOrElseUpdate" => {
                    let d = "(Ljava/lang/Object;Lscala/Function0;)Ljava/lang/Object;";
                    asm.invokeinterface("scala/collection/mutable/MapOps", "getOrElseUpdate", d);
                    maybe_unbox_erased_result(asm, ctx, d, result_ty);
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mutable_set(&owner) {
            match name {
                "contains" => {
                    asm.invokeinterface(
                        "scala/collection/SetOps",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "+=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Growable",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "-=" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/Shrinkable",
                        "-=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Shrinkable;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "remove" => {
                    asm.invokeinterface(
                        "scala/collection/mutable/SetOps",
                        "remove",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "nonEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "nonEmpty", "()Z");
                    return;
                }
                "clear" => {
                    asm.invokeinterface("scala/collection/mutable/Clearable", "clear", "()V");
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "map" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "map",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "filter" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOps",
                        "filter",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    checkcast_to(asm, ctx, result_ty, "scala/collection/mutable/Set");
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "toSeq" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toSeq",
                        "()Lscala/collection/immutable/Seq;",
                    );
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_coll_iterable(&owner) {
            match name {
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "size" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "size", "()I");
                    return;
                }
                "isEmpty" => {
                    asm.invokeinterface("scala/collection/IterableOnceOps", "isEmpty", "()Z");
                    return;
                }
                "iterator" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnce",
                        "iterator",
                        "()Lscala/collection/Iterator;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_mapview(&owner) {
            match name {
                "mapValues" => {
                    asm.invokeinterface(
                        "scala/collection/MapOps",
                        "mapValues",
                        "(Lscala/Function1;)Lscala/collection/MapView;",
                    );
                    return;
                }
                "toList" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toList",
                        "()Lscala/collection/immutable/List;",
                    );
                    return;
                }
                "mkString" => {
                    let desc = mkstring_desc(ctx.st, id);
                    asm.invokeinterface("scala/collection/IterableOnceOps", "mkString", desc);
                    return;
                }
                "foreach" => {
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashmap(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "get" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "get",
                        "(Ljava/lang/Object;)Lscala/Option;",
                    );
                    return;
                }
                "update" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "update",
                        "(Ljava/lang/Object;Ljava/lang/Object;)V",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashMap",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashmap(&owner) {
            match name {
                "apply" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "apply",
                        "(Ljava/lang/Object;)Ljava/lang/Object;",
                    );
                    if let Some(ty) = result_ty {
                        if is_jvm_primitive(ty) && !is_unit_like(ty) {
                            emit_unbox(asm, ty);
                        } else if !is_unit_like(ty) {
                            let cls = jvm_desc(ctx.st, ty);
                            if let Some(inner) =
                                cls.strip_prefix('L').and_then(|s| s.strip_suffix(';'))
                            {
                                if inner != "java/lang/Object" {
                                    asm.checkcast(inner);
                                }
                            }
                        }
                    }
                    return;
                }
                "update" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "update",
                        "(Ljava/lang/Object;Ljava/lang/Object;)V",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashMap",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_hashset(&owner) {
            match name {
                "contains" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/HashSet",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_linkedhashset(&owner) {
            match name {
                "contains" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet",
                        "contains",
                        "(Ljava/lang/Object;)Z",
                    );
                    return;
                }
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet",
                        "+=",
                        "(Ljava/lang/Object;)Lscala/collection/mutable/Growable;",
                    );
                    return;
                }
                "foreach" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/LinkedHashSet",
                        "foreach",
                        "(Lscala/Function1;)V",
                    );
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_stringbuilder(&owner) {
            match name {
                // `+=(Char)` has no direct override on StringBuilder (only the
                // erased `Growable.$plus$eq(Object): Growable`); `addOne(Char)`
                // is a concrete, non-erased override with the same effect.
                "+=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/StringBuilder",
                        "addOne",
                        "(C)Lscala/collection/mutable/StringBuilder;",
                    );
                    return;
                }
                // `++=(String)` similarly has no direct override; `addAll(String)`
                // is the concrete, non-erased equivalent.
                "++=" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/StringBuilder",
                        "addAll",
                        "(Ljava/lang/String;)Lscala/collection/mutable/StringBuilder;",
                    );
                    return;
                }
                // `append` has a concrete overload per argument type; use the
                // descriptor computed from the resolved (overloaded) symbol.
                "append" | "insert" | "deleteCharAt" | "setLength" | "clear" | "isEmpty"
                | "nonEmpty" | "length" | "toString" | "result" | "charAt" | "apply" => {
                    asm.invokevirtual("scala/collection/mutable/StringBuilder", name, &desc);
                    return;
                }
                // `reverse` is inherited from `IndexedSeqOps` and erased to
                // `Object`; checkcast back to the declared `StringBuilder`.
                "reverse" => {
                    asm.invokevirtual(
                        "scala/collection/mutable/StringBuilder",
                        "reverse",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/mutable/StringBuilder");
                    return;
                }
                _ => {}
            }
        }
        if is_stdlib_range(&owner) || is_stdlib_numeric_range(&owner) {
            if name == "mkString" {
                asm.invokeinterface(
                    "scala/collection/IterableOnceOps",
                    "mkString",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                );
                return;
            }
        }
        if is_stdlib_range(&owner) {
            match name {
                // `sum`/`min`/`max` take an implicit `Numeric`/`Ordering`
                // instance; Range's own overrides return `int` directly
                // (not the generic erased `Object`), so no checkcast/unbox
                // is needed after pushing the `Int` singleton.
                "sum" => {
                    asm.getstatic(
                        "scala/math/Numeric$IntIsIntegral$",
                        "MODULE$",
                        "Lscala/math/Numeric$IntIsIntegral$;",
                    );
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        "sum",
                        "(Lscala/math/Numeric;)I",
                    );
                    return;
                }
                // `product` has no `int`-returning override on `Range`
                // itself (only the generic `IterableOnceOps` default),
                // unlike `sum`/`min`/`max`.
                "product" => {
                    asm.getstatic(
                        "scala/math/Numeric$IntIsIntegral$",
                        "MODULE$",
                        "Lscala/math/Numeric$IntIsIntegral$;",
                    );
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "product",
                        "(Lscala/math/Numeric;)Ljava/lang/Object;",
                    );
                    emit_unbox(asm, &Type::Int);
                    return;
                }
                "min" | "max" => {
                    asm.getstatic(
                        "scala/math/Ordering$Int$",
                        "MODULE$",
                        "Lscala/math/Ordering$Int$;",
                    );
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        name,
                        "(Lscala/math/Ordering;)I",
                    );
                    return;
                }
                // `filter`/`filterNot`/`flatMap`/`zipWithIndex` only have the
                // generic `Object`-erased override on `Range` (no specific
                // `IndexedSeq`-returning bridge, unlike `map`/`take`/`drop`).
                "filter" | "filterNot" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        name,
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                "flatMap" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        "flatMap",
                        "(Lscala/Function1;)Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                "zipWithIndex" => {
                    asm.invokevirtual(
                        "scala/collection/immutable/Range",
                        "zipWithIndex",
                        "()Ljava/lang/Object;",
                    );
                    asm.checkcast("scala/collection/immutable/IndexedSeq");
                    return;
                }
                // `toArray` is only the `IterableOnceOps` generic default.
                "toArray" => {
                    asm.getstatic(
                        "scala/reflect/ClassTag$",
                        "MODULE$",
                        "Lscala/reflect/ClassTag$;",
                    );
                    asm.invokevirtual(
                        "scala/reflect/ClassTag$",
                        "Int",
                        "()Lscala/reflect/ManifestFactory$IntManifest;",
                    );
                    asm.invokeinterface(
                        "scala/collection/IterableOnceOps",
                        "toArray",
                        "(Lscala/reflect/ClassTag;)Ljava/lang/Object;",
                    );
                    asm.checkcast("[I");
                    return;
                }
                _ => {}
            }
        }
    }
    if is_interface_sym(ctx.st, owner_id) {
        asm.invokeinterface(&owner, name, &desc);
    } else {
        asm.invokevirtual(&owner, name, &desc);
    }
    maybe_unbox_erased_result(asm, ctx, &desc, result_ty);
}

/// After loading a generic field (`Object` / type param), cast or unbox to the
/// tree's instantiated type so `name + arg._1` can `append(String)`.
fn maybe_cast_erased_load(asm: &mut Assembler, ctx: &EmitCtx, from: &Type, want: &Type) {
    if is_jvm_primitive(want) && !is_unit_like(want) && !is_jvm_primitive(from) {
        emit_unbox(asm, want);
        return;
    }
    if matches!(want, Type::String) && !matches!(from, Type::String) {
        asm.checkcast("java/lang/String");
        return;
    }
    if let Some(cn) = checkcast_internal(ctx.st, want) {
        let from_desc = jvm_desc(ctx.st, from);
        if from_desc == "Ljava/lang/Object;" {
            asm.checkcast(&cn);
        }
    }
}

/// Unconditional checkcast to `result_ty`'s own JVM class (falling back to
/// `fallback` when `result_ty` erases to something without a class name,
/// e.g. a raw type param). Used after invoking a mixin default method whose
/// *declared* return type (`Buffer`, `Growable`, `Shrinkable`, the bound `C`
/// of `SeqOps`, …) is narrower than the concrete collection type we model in
/// the prelude (`ArrayBuffer[A]`, `ListBuffer[A]`, …) — unlike
/// `maybe_unbox_erased_result`, this does not require the invoked
/// descriptor's return type to literally be `Ljava/lang/Object;`.
/// Picks the right `mkString` overload descriptor for the resolved symbol
/// `id` (0/1/3 `String` params — the typer already chose the overload; we
/// just need to know which arity to emit against `IterableOnceOps`).
fn mkstring_desc(st: &SymbolTable, id: SymbolId) -> &'static str {
    let n = match &st.get(id).ty {
        Type::Method { paramss, .. } => paramss.first().map(|p| p.len()).unwrap_or(0),
        _ => 0,
    };
    match n {
        0 => "()Ljava/lang/String;",
        1 => "(Ljava/lang/String;)Ljava/lang/String;",
        _ => "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
    }
}

fn checkcast_to(asm: &mut Assembler, ctx: &EmitCtx, result_ty: Option<&Type>, fallback: &str) {
    if let Some(ty) = result_ty {
        if let Some(cn) = checkcast_internal(ctx.st, ty) {
            asm.checkcast(&cn);
            return;
        }
    }
    asm.checkcast(fallback);
}

/// After a generic invoke that returns `Object`, unbox when the tree still has
/// a primitive (e.g. `Iterator.next` / `Option.get` as `Int`).
fn maybe_unbox_erased_result(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    desc: &str,
    result_ty: Option<&Type>,
) {
    let Some(ty) = result_ty else {
        return;
    };
    if !desc_returns_object(desc) {
        return;
    }
    if is_jvm_primitive(ty) && !is_unit_like(ty) {
        // Private-runtime Option/Iterator already emit unboxed shapes; the
        // library ABI erases those to Object.
        if ctx.library_abi {
            emit_unbox(asm, ty);
        }
        return;
    }
    if matches!(ty, Type::String) {
        asm.checkcast("java/lang/String");
        return;
    }
    if let Type::Array(elem) = ty {
        if is_concrete_array_elem(elem) {
            let d = jvm_desc(ctx.st, ty);
            asm.checkcast(&d);
        }
        return;
    }
    if let Some(cn) = checkcast_internal(ctx.st, ty) {
        asm.checkcast(&cn);
    }
}

/// Lambda captures (and similar) are stored as `Object`. Restore the JVM type
/// before the body uses the local (`iastore` needs `[I`, not `Object`).
fn emit_from_erased_object(asm: &mut Assembler, st: &SymbolTable, ty: &Type) {
    if is_jvm_primitive(ty) {
        emit_unbox(asm, ty);
        return;
    }
    if matches!(ty, Type::String) {
        asm.checkcast("java/lang/String");
        return;
    }
    if matches!(ty, Type::Array(_)) {
        asm.checkcast(&jvm_desc(st, ty));
        return;
    }
    if let Type::Class { sym, .. } = ty {
        let n = class_internal(st, *sym);
        if !n.is_empty() && n != "java/lang/Object" {
            asm.checkcast(&n);
        }
        return;
    }
    if matches!(ty, Type::Tuple(_)) {
        asm.checkcast("scala/Tuple2");
    }
}

fn desc_returns_object(desc: &str) -> bool {
    desc.rsplit_once(')')
        .map(|(_, ret)| ret == "Ljava/lang/Object;")
        .unwrap_or(false)
}

fn array_elem_ty(ty: &Type) -> Option<Type> {
    match ty {
        Type::Array(t) => Some((**t).clone()),
        Type::Named { name, args } if name == "Array" && args.len() == 1 => Some(args[0].clone()),
        _ => None,
    }
}

fn is_concrete_array_elem(elem: &Type) -> bool {
    matches!(
        elem,
        Type::Boolean
            | Type::Int
            | Type::Long
            | Type::Double
            | Type::Float
            | Type::Char
            | Type::Byte
            | Type::Short
            | Type::String
            | Type::Class { .. }
            | Type::ModuleRef(_)
    )
}

fn emit_newarray(asm: &mut Assembler, ctx: &EmitCtx, elem: &Type) {
    match elem {
        Type::Boolean => asm.newarray(4),
        Type::Char => asm.newarray(5),
        Type::Float => asm.newarray(6),
        Type::Double => asm.newarray(7),
        Type::Byte => asm.newarray(8),
        Type::Short => asm.newarray(9),
        Type::Int => asm.newarray(10),
        Type::Long => asm.newarray(11),
        Type::String => asm.anewarray("java/lang/String"),
        Type::Class { sym, .. } | Type::ModuleRef(sym) => {
            asm.anewarray(&class_internal(ctx.st, *sym));
        }
        _ => asm.anewarray("java/lang/Object"),
    }
}

/// `List` の JVM オーナー（scala-library 2.13.16）。
const LIST_CLS: &str = "scala/collection/immutable/List";
const ITERABLE_ONCE_OPS: &str = "scala/collection/IterableOnceOps";
const ITERABLE_OPS: &str = "scala/collection/IterableOps";
const SEQ_OPS: &str = "scala/collection/SeqOps";

/// invoke 後の後処理。
#[derive(Clone, Copy, PartialEq)]
enum ListPost {
    /// そのまま。
    None,
    /// erase された戻り値を `List` へ戻す。
    CastList,
    /// `Object` 戻りを結果型に合わせて unbox / checkcast する。
    Erased,
}

/// `prelude_seq.rs` が足した `List` のコアメンバの invoke。
///
/// descriptor は `javap -s -cp scala-library-2.13.16.jar` で確認したもの。
/// `List` 自身が持たないメンバは `IterableOnceOps` / `IterableOps` / `SeqOps`
/// の default メソッドなので invokeinterface で呼ぶ。
///
/// 扱わない名前では `false` を返し、呼び出し側の既定の invoke に任せる。
fn emit_list_core_member(
    asm: &mut Assembler,
    ctx: &EmitCtx,
    name: &str,
    id: SymbolId,
    result_ty: Option<&Type>,
) -> bool {
    let s = ctx.st.get(id);
    let nargs = match &s.ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().count(),
        _ => s.params.len(),
    };
    // (invokeinterface か, オーナー, JVM 名, descriptor, 後処理)
    let (iface, owner, jvm, desc, post): (bool, &str, &str, &str, ListPost) = match (name, nargs) {
        // --- List 自身の virtual（戻り値も List）
        ("map", 1) | ("flatMap", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("::", 1) => (
            false,
            LIST_CLS,
            "::",
            "(Ljava/lang/Object;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        // `indexWhere(p)` は `indexWhere(p, 0)`（既定引数）。
        ("indexWhere", 1) => {
            asm.iconst(0);
            (
                false,
                LIST_CLS,
                "indexWhere",
                "(Lscala/Function1;I)I",
                ListPost::None,
            )
        }
        ("filter", 1) | ("filterNot", 1) | ("takeWhile", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("take", 1) | ("takeRight", 1) => (
            false,
            LIST_CLS,
            name,
            "(I)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("slice", 2) => (
            false,
            LIST_CLS,
            "slice",
            "(II)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("reverse", 0) | ("toList", 0) => (
            false,
            LIST_CLS,
            name,
            "()Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("updated", 2) => (
            false,
            LIST_CLS,
            "updated",
            "(ILjava/lang/Object;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("splitAt", 1) => (
            false,
            LIST_CLS,
            "splitAt",
            "(I)Lscala/Tuple2;",
            ListPost::None,
        ),
        ("span", 1) | ("partition", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Lscala/Tuple2;",
            ListPost::None,
        ),
        ("forall", 1) | ("exists", 1) => (
            false,
            LIST_CLS,
            name,
            "(Lscala/Function1;)Z",
            ListPost::None,
        ),
        ("contains", 1) => (
            false,
            LIST_CLS,
            "contains",
            "(Ljava/lang/Object;)Z",
            ListPost::None,
        ),
        ("find", 1) => (
            false,
            LIST_CLS,
            "find",
            "(Lscala/Function1;)Lscala/Option;",
            ListPost::None,
        ),
        ("headOption", 0) => (
            false,
            LIST_CLS,
            "headOption",
            "()Lscala/Option;",
            ListPost::None,
        ),
        ("last", 0) => (
            false,
            LIST_CLS,
            "last",
            "()Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("foldLeft", 2) | ("foldRight", 2) => (
            false,
            LIST_CLS,
            name,
            "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        // --- List の virtual だが戻り値が erase される
        ("drop", 1) => (
            false,
            LIST_CLS,
            "drop",
            "(I)Lscala/collection/LinearSeq;",
            ListPost::CastList,
        ),
        ("dropWhile", 1) => (
            false,
            LIST_CLS,
            "dropWhile",
            "(Lscala/Function1;)Lscala/collection/LinearSeq;",
            ListPost::CastList,
        ),
        ("dropRight", 1) => (
            false,
            LIST_CLS,
            "dropRight",
            "(I)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("distinctBy", 1) => (
            false,
            LIST_CLS,
            "distinctBy",
            "(Lscala/Function1;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("sorted", 1) => (
            false,
            LIST_CLS,
            "sorted",
            "(Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("zip", 1) => (
            false,
            LIST_CLS,
            "zip",
            "(Lscala/collection/IterableOnce;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("zipWithIndex", 0) => (
            false,
            LIST_CLS,
            "zipWithIndex",
            "()Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("scanLeft", 2) => (
            false,
            LIST_CLS,
            "scanLeft",
            "(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        // --- 連結・追加。`++` / `:++` は `appendedAll`、`++:` は `prependedAll`。
        (":::", 1) => (
            false,
            LIST_CLS,
            ":::",
            "(Lscala/collection/immutable/List;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("+:", 1) => (
            false,
            LIST_CLS,
            "prepended",
            "(Ljava/lang/Object;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        (":+", 1) => (
            false,
            LIST_CLS,
            "appended",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("++", 1) | (":++", 1) | ("concat", 1) => (
            false,
            LIST_CLS,
            "appendedAll",
            "(Lscala/collection/IterableOnce;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        ("++:", 1) => (
            false,
            LIST_CLS,
            "prependedAll",
            "(Lscala/collection/IterableOnce;)Lscala/collection/immutable/List;",
            ListPost::None,
        ),
        // --- IterableOnceOps の default メソッド
        ("size", 0) => (true, ITERABLE_ONCE_OPS, "size", "()I", ListPost::None),
        ("nonEmpty", 0) => (true, ITERABLE_ONCE_OPS, "nonEmpty", "()Z", ListPost::None),
        ("count", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            "count",
            "(Lscala/Function1;)I",
            ListPost::None,
        ),
        ("mkString", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "mkString",
            "()Ljava/lang/String;",
            ListPost::None,
        ),
        ("mkString", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            "mkString",
            "(Ljava/lang/String;)Ljava/lang/String;",
            ListPost::None,
        ),
        ("mkString", 3) => (
            true,
            ITERABLE_ONCE_OPS,
            "mkString",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            ListPost::None,
        ),
        ("sum", 1) | ("product", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/math/Numeric;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("min", 1) | ("max", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("minBy", 2) | ("maxBy", 2) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("reduce", 1) | ("reduceLeft", 1) | ("reduceRight", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            name,
            "(Lscala/Function2;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("toArray", 1) => (
            true,
            ITERABLE_ONCE_OPS,
            "toArray",
            "(Lscala/reflect/ClassTag;)Ljava/lang/Object;",
            ListPost::Erased,
        ),
        ("toSet", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "toSet",
            "()Lscala/collection/immutable/Set;",
            ListPost::None,
        ),
        ("toVector", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "toVector",
            "()Lscala/collection/immutable/Vector;",
            ListPost::None,
        ),
        ("toSeq", 0) => (
            true,
            ITERABLE_ONCE_OPS,
            "toSeq",
            "()Lscala/collection/immutable/Seq;",
            ListPost::None,
        ),
        // --- IterableOps の default メソッド
        ("init", 0) => (
            true,
            ITERABLE_OPS,
            "init",
            "()Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("lastOption", 0) => (
            true,
            ITERABLE_OPS,
            "lastOption",
            "()Lscala/Option;",
            ListPost::None,
        ),
        ("groupBy", 1) => (
            true,
            ITERABLE_OPS,
            "groupBy",
            "(Lscala/Function1;)Lscala/collection/immutable/Map;",
            ListPost::None,
        ),
        ("grouped", 1) | ("sliding", 1) => (
            true,
            ITERABLE_OPS,
            name,
            "(I)Lscala/collection/Iterator;",
            ListPost::None,
        ),
        ("sliding", 2) => (
            true,
            ITERABLE_OPS,
            "sliding",
            "(II)Lscala/collection/Iterator;",
            ListPost::None,
        ),
        // --- SeqOps の default メソッド
        ("distinct", 0) => (
            true,
            SEQ_OPS,
            "distinct",
            "()Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("sortBy", 2) => (
            true,
            SEQ_OPS,
            "sortBy",
            "(Lscala/Function1;Lscala/math/Ordering;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("sortWith", 1) => (
            true,
            SEQ_OPS,
            "sortWith",
            "(Lscala/Function2;)Ljava/lang/Object;",
            ListPost::CastList,
        ),
        ("indexOf", 1) => (
            true,
            SEQ_OPS,
            "indexOf",
            "(Ljava/lang/Object;)I",
            ListPost::None,
        ),
        ("endsWith", 1) => (
            true,
            SEQ_OPS,
            "endsWith",
            "(Lscala/collection/Iterable;)Z",
            ListPost::None,
        ),
        // `startsWith(that)` は `startsWith(that, 0)`（既定引数）。
        ("startsWith", 1) => {
            asm.iconst(0);
            (
                true,
                SEQ_OPS,
                "startsWith",
                "(Lscala/collection/IterableOnce;I)Z",
                ListPost::None,
            )
        }
        _ => return false,
    };
    if iface {
        asm.invokeinterface(owner, jvm, desc);
    } else {
        asm.invokevirtual(owner, jvm, desc);
    }
    match post {
        ListPost::None => {}
        ListPost::CastList => asm.checkcast(LIST_CLS),
        ListPost::Erased => maybe_unbox_erased_result(asm, ctx, desc, result_ty),
    }
    true
}

fn is_stdlib_list(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/List"
            | "scala/collection/immutable/$colon$colon"
            | "scala/collection/immutable/Nil$"
    )
}

fn is_stdlib_option(owner: &str) -> bool {
    matches!(owner, "scala/Option" | "scala/Some" | "scala/None$")
}

fn is_list_module_owner(owner: &str) -> bool {
    owner == "scala/collection/immutable/List$"
}

fn is_stdlib_map(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Map"
            | "scala/collection/immutable/Map$EmptyMap$"
            | "scala/collection/immutable/HashMap"
    )
}

fn is_stdlib_map_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Map$"
}

fn is_stdlib_vector(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Vector"
            | "scala/collection/immutable/Vector0$"
            | "scala/collection/immutable/Vector1"
            | "scala/collection/immutable/Vector2"
            | "scala/collection/immutable/Vector3"
    )
}

fn is_stdlib_vector_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Vector$"
}

fn is_stdlib_indexedseq(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/IndexedSeq" | "scala/collection/IndexedSeq"
    )
}

fn is_stdlib_indexedseq_module(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/IndexedSeq$" | "scala/collection/IndexedSeq$"
    )
}

fn is_stdlib_queue(owner: &str) -> bool {
    owner == "scala/collection/immutable/Queue"
}

fn is_stdlib_queue_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Queue$"
}

fn is_stdlib_arraybuffer(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayBuffer"
}

fn is_stdlib_arraybuffer_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayBuffer$"
}

fn is_stdlib_mutable_map(owner: &str) -> bool {
    owner == "scala/collection/mutable/Map"
}

fn is_stdlib_mutable_map_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/Map$"
}

fn is_stdlib_mutable_set(owner: &str) -> bool {
    owner == "scala/collection/mutable/Set"
}

fn is_stdlib_mutable_set_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/Set$"
}

fn is_stdlib_coll_iterable(owner: &str) -> bool {
    owner == "scala/collection/Iterable"
}

fn is_stdlib_mapview(owner: &str) -> bool {
    owner == "scala/collection/MapView"
}

fn is_stdlib_arraydeque(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayDeque"
}

fn is_stdlib_arraydeque_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/ArrayDeque$"
}

fn is_stdlib_listbuffer(owner: &str) -> bool {
    owner == "scala/collection/mutable/ListBuffer"
}

fn is_stdlib_listbuffer_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/ListBuffer$"
}

fn is_stdlib_stringbuilder(owner: &str) -> bool {
    owner == "scala/collection/mutable/StringBuilder"
}

fn is_stdlib_hashmap(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashMap"
}

fn is_stdlib_hashmap_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashMap$"
}

fn is_stdlib_hashset(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashSet"
}

fn is_stdlib_hashset_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/HashSet$"
}

fn is_stdlib_linkedhashmap(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashMap"
}

fn is_stdlib_linkedhashmap_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashMap$"
}

fn is_stdlib_linkedhashset(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashSet"
}

fn is_stdlib_linkedhashset_module(owner: &str) -> bool {
    owner == "scala/collection/mutable/LinkedHashSet$"
}

fn emit_long_numeric_range(asm: &mut Assembler, inclusive: bool) {
    // stack: start (J), end (J). RichLong has no to$extension; IntegralProxy
    // default builds a real NumericRange[Long] via Numeric$LongIsIntegral$.
    emit_box(asm, &Type::Long);
    // start (J), boxedEnd
    asm.dup_x2();
    asm.pop();
    emit_box(asm, &Type::Long);
    asm.swap();
    asm.getstatic(
        "scala/collection/immutable/NumericRange$",
        "MODULE$",
        "Lscala/collection/immutable/NumericRange$;",
    );
    asm.dup_x2();
    asm.pop();
    asm.lconst(1);
    emit_box(asm, &Type::Long);
    asm.getstatic(
        "scala/math/Numeric$LongIsIntegral$",
        "MODULE$",
        "Lscala/math/Numeric$LongIsIntegral$;",
    );
    if inclusive {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "inclusive",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Inclusive;",
        );
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "apply",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Exclusive;",
        );
    }
}

fn emit_integral_numeric_range(asm: &mut Assembler, elem: &Type, inclusive: bool) {
    // stack: start, end (integral as int)
    let integral = match elem {
        Type::Short => "scala/math/Numeric$ShortIsIntegral$",
        Type::Char => "scala/math/Numeric$CharIsIntegral$",
        _ => "scala/math/Numeric$ByteIsIntegral$",
    };
    asm.swap();
    narrow_integral(asm, elem);
    emit_box(asm, elem);
    asm.swap();
    narrow_integral(asm, elem);
    emit_box(asm, elem);
    asm.getstatic(
        "scala/collection/immutable/NumericRange$",
        "MODULE$",
        "Lscala/collection/immutable/NumericRange$;",
    );
    asm.dup_x2();
    asm.pop();
    asm.iconst(1);
    narrow_integral(asm, elem);
    emit_box(asm, elem);
    asm.getstatic(integral, "MODULE$", &format!("L{integral};"));
    if inclusive {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "inclusive",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Inclusive;",
        );
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/NumericRange$",
            "apply",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Lscala/math/Integral;)Lscala/collection/immutable/NumericRange$Exclusive;",
        );
    }
}

fn narrow_integral(asm: &mut Assembler, elem: &Type) {
    match elem {
        Type::Short => asm.i2s(),
        Type::Char => asm.i2c(),
        _ => asm.i2b(),
    }
}

fn is_stdlib_range(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Range" | "scala/collection/immutable/Range$Inclusive"
    )
}

fn is_stdlib_numeric_range(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/NumericRange"
            | "scala/collection/immutable/NumericRange$Inclusive"
            | "scala/collection/immutable/NumericRange$Exclusive"
    )
}

fn is_stdlib_set(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Set"
            | "scala/collection/immutable/Set$EmptySet$"
            | "scala/collection/immutable/Set$Set1"
            | "scala/collection/immutable/Set$Set2"
            | "scala/collection/immutable/Set$Set3"
            | "scala/collection/immutable/Set$Set4"
            | "scala/collection/immutable/HashSet"
            | "scala/collection/immutable/SortedSet"
            | "scala/collection/immutable/TreeSet"
    )
}

fn is_stdlib_set_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Set$"
}

fn is_stdlib_sortedset_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/SortedSet$"
}

fn is_stdlib_treeset_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/TreeSet$"
}

fn is_stdlib_sortedmap(owner: &str) -> bool {
    owner == "scala/collection/immutable/SortedMap"
}

fn is_stdlib_sortedmap_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/SortedMap$"
}

fn is_stdlib_treemap(owner: &str) -> bool {
    owner == "scala/collection/immutable/TreeMap"
}

fn is_stdlib_treemap_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/TreeMap$"
}

fn is_stdlib_bitset(owner: &str) -> bool {
    owner == "scala/collection/immutable/BitSet"
}

fn is_stdlib_bitset_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/BitSet$"
}

fn is_stdlib_seq(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/Seq" | "scala/collection/Seq"
    )
}

fn is_stdlib_seq_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/Seq$"
}

fn is_stdlib_lazylist(owner: &str) -> bool {
    matches!(
        owner,
        "scala/collection/immutable/LazyList" | "scala/collection/immutable/LazyList$Empty$"
    )
}

fn is_stdlib_lazylist_module(owner: &str) -> bool {
    owner == "scala/collection/immutable/LazyList$"
}

fn is_stdlib_either(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/Either" | "scala/util/Left" | "scala/util/Right"
    )
}

fn is_stdlib_either_module(owner: &str) -> bool {
    matches!(owner, "scala/util/Left$" | "scala/util/Right$")
}

fn is_stdlib_breaks(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/control/Breaks" | "scala/util/control/Breaks$"
    )
}

fn is_stdlib_try(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/Try" | "scala/util/Success" | "scala/util/Failure"
    )
}

fn is_stdlib_try_module(owner: &str) -> bool {
    matches!(
        owner,
        "scala/util/Try$" | "scala/util/Success$" | "scala/util/Failure$"
    )
}

fn gen_java_varargs_array(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    elem: &Type,
) {
    let n = args.len() as i32;
    asm.iconst(n);
    match elem {
        Type::String => asm.anewarray("java/lang/String"),
        Type::Class { sym, .. } | Type::ModuleRef(sym) => {
            asm.anewarray(&class_internal(ctx.st, *sym));
        }
        _ => asm.anewarray("java/lang/Object"),
    }
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
        asm.aastore();
    }
}

fn gen_wrap_varargs(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    elem: &Type,
) {
    let n = args.len() as i32;
    let all_int = !args.is_empty()
        && args.iter().all(|a| matches!(a.ty, Type::Int))
        && matches!(
            elem,
            Type::Int | Type::Any | Type::AnyRef | Type::TypeParam(_)
        );
    let all_unit = !args.is_empty() && args.iter().all(is_unit_varargs_elem);
    asm.getstatic(
        "scala/runtime/ScalaRunTime$",
        "MODULE$",
        "Lscala/runtime/ScalaRunTime$;",
    );
    asm.iconst(n);
    if all_int {
        asm.newarray(10); // T_INT
        for (i, a) in args.iter().enumerate() {
            asm.dup();
            asm.iconst(i as i32);
            gen_expr(asm, frame, ctx, a);
            asm.iastore();
        }
        asm.invokevirtual(
            "scala/runtime/ScalaRunTime$",
            "wrapIntArray",
            "([I)Lscala/collection/immutable/ArraySeq;",
        );
    } else if all_unit && ctx.library_abi {
        // nsc `Array((), ())` uses wrapUnitArray of BoxedUnit.UNIT, not null.
        // Erasure may wrap some elems in `$box`, so treat those as Unit too.
        asm.anewarray("scala/runtime/BoxedUnit");
        for (i, a) in args.iter().enumerate() {
            asm.dup();
            asm.iconst(i as i32);
            gen_varargs_elem(asm, frame, ctx, a);
            asm.aastore();
        }
        asm.invokevirtual(
            "scala/runtime/ScalaRunTime$",
            "wrapUnitArray",
            "([Lscala/runtime/BoxedUnit;)Lscala/collection/immutable/ArraySeq;",
        );
    } else {
        asm.anewarray("java/lang/Object");
        for (i, a) in args.iter().enumerate() {
            asm.dup();
            asm.iconst(i as i32);
            gen_varargs_elem(asm, frame, ctx, a);
            asm.aastore();
        }
        asm.invokevirtual(
            "scala/runtime/ScalaRunTime$",
            "wrapRefArray",
            "([Ljava/lang/Object;)Lscala/collection/immutable/ArraySeq;",
        );
    }
}

fn gen_call_args(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    param_tys: &[Type],
    box_prims: bool,
    java_varargs: bool,
    method: SymbolId,
) {
    let param_ids: Vec<SymbolId> = if method.is_none() {
        Vec::new()
    } else {
        ctx.st.get(method).params.clone()
    };
    let load_arg = |asm: &mut Assembler, frame: &mut Frame, i: usize, a: &Tree| {
        let callee_synthetic =
            !method.is_none() && ctx.st.get(method).flags.contains(Flags::SYNTHETIC);
        if callee_synthetic && param_ids.get(i).is_some_and(|p| ctx.boxed_vars.contains(p)) {
            let id = match &a.kind {
                TreeKind::Ident { .. } => a.sym,
                TreeKind::Typed { expr, .. } => expr.sym,
                _ => SymbolId::NONE,
            };
            if let Some((slot, _)) = frame.get(id) {
                load(asm, slot, JvmSort::Ref);
                return;
            }
        }
        gen_expr(asm, frame, ctx, a);
        if box_prims {
            let pty = param_tys.get(i).unwrap_or(&a.ty);
            if is_jvm_primitive(&a.ty) && !is_unit_like(&a.ty) && !is_jvm_primitive(pty) {
                emit_box(asm, &a.ty);
            } else if matches!(pty, Type::Array(_)) {
                asm.checkcast(&jvm_desc(ctx.st, pty));
            }
        }
    };
    let rep_idx = param_tys
        .iter()
        .position(|p| matches!(p, Type::Repeated(_)));
    let Some(ri) = rep_idx else {
        for (i, a) in args.iter().enumerate() {
            load_arg(asm, frame, i, a);
        }
        return;
    };
    let n_after = param_tys.len() - ri - 1;
    let n_var = args.len().saturating_sub(ri + n_after);
    for a in &args[..ri.min(args.len())] {
        gen_expr(asm, frame, ctx, a);
    }
    let var_end = (ri + n_var).min(args.len());
    let var_args = if ri <= var_end {
        &args[ri..var_end]
    } else {
        &[]
    };
    let elem = match &param_tys[ri] {
        Type::Repeated(t) => t.as_ref(),
        _ => &Type::Any,
    };
    // `f(xs: _*)` already has the sequence; nothing to wrap.
    let spliced = var_args.len() == 1 && matches!(var_args[0].ty, Type::Repeated(_));
    if spliced {
        let inner = match &var_args[0].kind {
            TreeKind::Typed { expr, .. } => expr.as_ref(),
            _ => &var_args[0],
        };
        gen_expr(asm, frame, ctx, inner);
    } else if java_varargs {
        gen_java_varargs_array(asm, frame, ctx, var_args, elem);
    } else {
        gen_wrap_varargs(asm, frame, ctx, var_args, elem);
    }
    for a in &args[var_end..] {
        gen_expr(asm, frame, ctx, a);
    }
}

fn emit_array_load(asm: &mut Assembler, elem: &Type) {
    match elem {
        Type::Int | Type::Boolean => asm.iaload(),
        _ => asm.aaload(),
    }
}

fn emit_array_store(asm: &mut Assembler, arr_ty: &Type) {
    match arr_ty {
        Type::Array(elem) if matches!(elem.as_ref(), Type::Int | Type::Boolean) => {
            asm.iastore();
        }
        _ => asm.aastore(),
    }
}

fn load_predef_module(asm: &mut Assembler) {
    asm.getstatic("scala/Predef$", "MODULE$", "Lscala/Predef$;");
}

fn emit_boxed_unit(asm: &mut Assembler) {
    asm.getstatic(
        "scala/runtime/BoxedUnit",
        "UNIT",
        "Lscala/runtime/BoxedUnit;",
    );
}

/// Unit literals, and `$box(unit)` inserted by erasure (whose result type is
/// Object). Used so `Array((), ())` still takes the wrapUnitArray path.
fn is_unit_varargs_elem(tree: &Tree) -> bool {
    if matches!(tree.ty.widen_constant(), Type::Unit | Type::NoType) {
        return true;
    }
    match &tree.kind {
        TreeKind::Typed { expr, .. } | TreeKind::Block { expr, .. } => is_unit_varargs_elem(expr),
        TreeKind::TypeApply { fun, .. } => is_unit_varargs_elem(fun),
        TreeKind::Apply { fun, args } => {
            peel_fun(fun).name() == Some("$box") && args.first().is_some_and(is_unit_varargs_elem)
        }
        _ => false,
    }
}

fn gen_varargs_elem(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, a: &Tree) {
    if ctx.library_abi && is_unit_varargs_elem(a) {
        if unit_leaves_boxed_ref(a, ctx.st) {
            gen_expr(asm, frame, ctx, a);
        } else {
            gen_expr(asm, frame, ctx, a);
            emit_boxed_unit(asm);
        }
    } else {
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
    }
}

/// True when a Unit-typed expression already left a boxed ref (`BoxedUnit` or
/// `null`) on the stack — ArrayOps / generic `T` erased to Object / `$box`.
/// Unit literals leave nothing and need `BoxedUnit.UNIT`.
fn unit_leaves_boxed_ref(tree: &Tree, st: &SymbolTable) -> bool {
    match &tree.kind {
        TreeKind::Typed { expr, .. } => unit_leaves_boxed_ref(expr, st),
        TreeKind::TypeApply { fun, .. } => unit_leaves_boxed_ref(fun, st),
        TreeKind::Block { expr, .. } => unit_leaves_boxed_ref(expr, st),
        TreeKind::Apply { fun, .. } => {
            peel_fun(fun).name() == Some("$box") || method_erases_unit_to_ref(fun, st)
        }
        TreeKind::Select { .. } | TreeKind::Ident { .. } => method_erases_unit_to_ref(tree, st),
        _ => false,
    }
}

fn method_erases_unit_to_ref(fun: &Tree, st: &SymbolTable) -> bool {
    match &fun.kind {
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
            return method_erases_unit_to_ref(fun, st);
        }
        _ => {}
    }
    if fun.sym.is_none() {
        return false;
    }
    let s = st.get(fun.sym);
    if st.get(s.owner).name == "ArrayOps" {
        return true;
    }
    match &s.ty {
        Type::Method { ret, .. } | Type::Function { ret, .. } => {
            matches!(ret.as_ref(), Type::TypeParam(_))
        }
        Type::TypeParam(_) => true,
        _ => false,
    }
}

fn emit_predef_nyi(asm: &mut Assembler) {
    load_predef_module(asm);
    asm.invokevirtual("scala/Predef$", "???", "()Lscala/runtime/Nothing$;");
}

fn gen_predef_println(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    newline: bool,
) {
    if args.is_empty() {
        load_predef_module(asm);
        if newline {
            asm.invokevirtual("scala/Predef$", "println", "()V");
        } else {
            asm.aconst_null();
            asm.invokevirtual("scala/Predef$", "print", "(Ljava/lang/Object;)V");
        }
        return;
    }
    // Evaluate the argument first so a comparison's branch target is not
    // sitting under `MODULE$` (Java 6 inference verifier / later StackMap).
    let a = &args[0];
    gen_expr(asm, frame, ctx, a);
    if is_unit_like(&a.ty) {
        // nsc Predef.println(x: Any) prints BoxedUnit / null, not a fake "()".
        if !unit_leaves_boxed_ref(a, ctx.st) {
            emit_boxed_unit(asm);
        }
    } else if is_jvm_primitive(&a.ty) {
        emit_box(asm, &a.ty);
    }
    load_predef_module(asm);
    asm.swap();
    let name = if newline { "println" } else { "print" };
    asm.invokevirtual("scala/Predef$", name, "(Ljava/lang/Object;)V");
}

fn gen_predef_poly(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    result_ty: &Type,
    name: &str,
) {
    let Some(a) = args.first() else {
        load_predef_module(asm);
        asm.aconst_null();
        asm.invokevirtual(
            "scala/Predef$",
            name,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        );
        if is_unit_like(result_ty) {
            asm.pop();
        }
        return;
    };
    gen_expr(asm, frame, ctx, a);
    if is_unit_like(&a.ty) {
        asm.aconst_null();
    } else if is_jvm_primitive(&a.ty) {
        emit_box(asm, &a.ty);
    }
    load_predef_module(asm);
    asm.swap();
    asm.invokevirtual(
        "scala/Predef$",
        name,
        "(Ljava/lang/Object;)Ljava/lang/Object;",
    );
    if is_unit_like(result_ty) {
        asm.pop();
    }
}

fn gen_predef_assert_require(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    is_assert: bool,
) {
    let Some(cond) = args.first() else {
        return;
    };
    gen_expr(asm, frame, ctx, cond);
    let name = if is_assert { "assert" } else { "require" };
    if let Some(msg) = args.get(1) {
        gen_expr(asm, frame, ctx, msg);
        load_predef_module(asm);
        // cond, msg, MODULE$ → MODULE$, cond, msg
        asm.dup_x2();
        asm.pop();
        asm.invokevirtual("scala/Predef$", name, "(ZLscala/Function0;)V");
    } else {
        load_predef_module(asm);
        asm.swap();
        asm.invokevirtual("scala/Predef$", name, "(Z)V");
    }
}

fn is_list_unapply_seq(st: &SymbolTable, uid: SymbolId) -> bool {
    let s = st.get(uid);
    s.name == "unapplySeq" && is_list_module_owner(&class_internal(st, s.owner))
}

fn is_arrow_assoc_arrow(ctx: &EmitCtx, fun: &Tree) -> bool {
    if fun.name() != Some("->") {
        return false;
    }
    if fun.sym.is_none() {
        return true;
    }
    class_internal(ctx.st, ctx.st.get(fun.sym).owner).contains("ArrowAssoc")
}

fn gen_tuple2_arrow(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
) {
    asm.new_obj("scala/Tuple2");
    asm.dup();
    gen_receiver(asm, frame, ctx, fun);
    if let Some(a) = args.first() {
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
    } else {
        asm.aconst_null();
    }
    asm.invokespecial(
        "scala/Tuple2",
        "<init>",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
    );
}

fn gen_assert_require(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    is_assert: bool,
) {
    let Some(cond) = args.first() else {
        return;
    };
    gen_expr(asm, frame, ctx, cond);
    let ok = asm.fresh_label();
    asm.ifne(ok);
    let cls = if is_assert {
        "java/lang/AssertionError"
    } else {
        "java/lang/IllegalArgumentException"
    };
    asm.new_obj(cls);
    asm.dup();
    if let Some(msg) = args.get(1) {
        gen_expr(asm, frame, ctx, msg);
        if matches!(&msg.ty, Type::Function { .. }) {
            asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
        } else if is_jvm_primitive(&msg.ty) {
            emit_box(asm, &msg.ty);
        }
        asm.invokevirtual("java/lang/Object", "toString", "()Ljava/lang/String;");
        if is_assert {
            asm.invokespecial(cls, "<init>", "(Ljava/lang/Object;)V");
        } else {
            asm.invokespecial(cls, "<init>", "(Ljava/lang/String;)V");
        }
    } else if is_assert {
        asm.invokespecial(cls, "<init>", "()V");
    } else {
        asm.ldc_string("requirement failed");
        asm.invokespecial(cls, "<init>", "(Ljava/lang/String;)V");
    }
    asm.athrow();
    asm.mark(ok);
}

fn emit_box(asm: &mut Assembler, ty: &Type) {
    emit_box_inner(asm, &ty.widen_constant())
}

fn emit_box_inner(asm: &mut Assembler, ty: &Type) {
    match ty {
        Type::Int => {
            asm.invokestatic("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;");
        }
        Type::Boolean => {
            asm.invokestatic("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;");
        }
        Type::Byte => {
            asm.invokestatic("java/lang/Byte", "valueOf", "(B)Ljava/lang/Byte;");
        }
        Type::Short => {
            asm.invokestatic("java/lang/Short", "valueOf", "(S)Ljava/lang/Short;");
        }
        Type::Long => {
            asm.invokestatic("java/lang/Long", "valueOf", "(J)Ljava/lang/Long;");
        }
        Type::Double => {
            asm.invokestatic("java/lang/Double", "valueOf", "(D)Ljava/lang/Double;");
        }
        Type::Char => {
            asm.invokestatic("java/lang/Character", "valueOf", "(C)Ljava/lang/Character;");
        }
        Type::Float => {
            asm.invokestatic("java/lang/Float", "valueOf", "(F)Ljava/lang/Float;");
        }
        Type::Unit | Type::NoType => {
            asm.aconst_null();
        }
        _ => {}
    }
}

fn emit_unbox(asm: &mut Assembler, ty: &Type) {
    emit_unbox_inner(asm, &ty.widen_constant())
}

fn emit_unbox_inner(asm: &mut Assembler, ty: &Type) {
    match ty {
        Type::Int => {
            asm.checkcast("java/lang/Integer");
            asm.invokevirtual("java/lang/Integer", "intValue", "()I");
        }
        Type::Boolean => {
            asm.checkcast("java/lang/Boolean");
            asm.invokevirtual("java/lang/Boolean", "booleanValue", "()Z");
        }
        Type::Byte => {
            asm.checkcast("java/lang/Byte");
            asm.invokevirtual("java/lang/Byte", "byteValue", "()B");
        }
        Type::Short => {
            asm.checkcast("java/lang/Short");
            asm.invokevirtual("java/lang/Short", "shortValue", "()S");
        }
        Type::Long => {
            asm.checkcast("java/lang/Long");
            asm.invokevirtual("java/lang/Long", "longValue", "()J");
        }
        Type::Double => {
            asm.checkcast("java/lang/Double");
            asm.invokevirtual("java/lang/Double", "doubleValue", "()D");
        }
        Type::Char => {
            asm.checkcast("java/lang/Character");
            asm.invokevirtual("java/lang/Character", "charValue", "()C");
        }
        Type::Float => {
            asm.checkcast("java/lang/Float");
            asm.invokevirtual("java/lang/Float", "floatValue", "()F");
        }
        Type::String => {
            asm.checkcast("java/lang/String");
        }
        Type::Class { .. } | Type::ModuleRef(_) => {
            // leave as Object; checkcast when we have an internal name
        }
        Type::Unit | Type::NoType => {
            asm.pop();
        }
        _ => {}
    }
}

fn gen_function_apply(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, fun);
    let n = match &fun.ty {
        Type::Function { params, .. } => params.len(),
        _ => args.len(),
    };
    let param_tys = match &fun.ty {
        Type::Function { params, .. } => params.clone(),
        _ => args.iter().map(|a| a.ty.clone()).collect(),
    };
    for (i, a) in args.iter().enumerate() {
        gen_expr(asm, frame, ctx, a);
        let pty = param_tys.get(i).unwrap_or(&a.ty);
        if is_jvm_primitive(pty) || is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        }
    }
    let iface = format!("scala/Function{n}");
    let mut desc = String::from("(");
    for _ in 0..n {
        desc.push_str("Ljava/lang/Object;");
    }
    desc.push_str(")Ljava/lang/Object;");
    asm.invokeinterface(&iface, "apply", &desc);
    if is_jvm_primitive(result_ty) {
        emit_unbox(asm, result_ty);
    } else if matches!(result_ty, Type::String) {
        emit_unbox(asm, result_ty);
    } else if let Type::Class { sym, .. } = result_ty {
        let n = class_internal(ctx.st, *sym);
        if !n.is_empty() {
            asm.checkcast(&n);
        }
    }
}

fn is_jvm_primitive(ty: &Type) -> bool {
    matches!(
        ty.widen_constant(),
        Type::Int
            | Type::Long
            | Type::Double
            | Type::Boolean
            | Type::Char
            | Type::Float
            | Type::Byte
            | Type::Short
            | Type::Unit
    )
}

fn collect_boxed_vars(tree: &Tree, st: &SymbolTable) -> HashSet<SymbolId> {
    let mut out = HashSet::new();
    walk_boxed_vars(tree, st, &mut out);
    // A `var` captured by a class defined inside a method is shared with the
    // enclosing method, exactly like one captured by a lambda.
    for s in &st.symbols {
        for c in &s.captures {
            if st.get(*c).flags.contains(Flags::MUTABLE) {
                out.insert(*c);
            }
        }
    }
    out
}

fn walk_boxed_vars(tree: &Tree, st: &SymbolTable, out: &mut HashSet<SymbolId>) {
    match &tree.kind {
        TreeKind::PackageDef { stats, .. } => {
            for s in stats {
                walk_boxed_vars(s, st, out);
            }
        }
        TreeKind::ClassDef {
            vparamss, impl_, ..
        } => {
            for p in vparamss.iter().flatten() {
                walk_boxed_vars(p, st, out);
            }
            for p in &impl_.parents {
                walk_boxed_vars(p, st, out);
            }
            for s in &impl_.body {
                walk_boxed_vars(s, st, out);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for p in &impl_.parents {
                walk_boxed_vars(p, st, out);
            }
            for s in &impl_.body {
                walk_boxed_vars(s, st, out);
            }
        }
        TreeKind::DefDef {
            vparamss, tpt, rhs, ..
        } => {
            let synthetic = def_is_synthetic(st, tree);
            for p in vparamss.iter().flatten() {
                if synthetic && !p.sym.is_none() && st.get(p.sym).flags.contains(Flags::MUTABLE) {
                    out.insert(p.sym);
                }
                walk_boxed_vars(p, st, out);
            }
            walk_boxed_vars(tpt, st, out);
            walk_boxed_vars(rhs, st, out);
        }
        TreeKind::Function { vparams, body } => {
            let mut bound = HashSet::new();
            for p in vparams {
                if !p.sym.is_none() {
                    bound.insert(p.sym);
                }
                walk_boxed_vars(p, st, out);
            }
            let mut free = Vec::new();
            collect_free(body, &bound, &mut free, st);
            for id in free {
                let s = st.get(id);
                if s.flags.contains(Flags::MUTABLE)
                    && matches!(st.get(s.owner).kind, SymKind::Method)
                {
                    out.insert(id);
                }
            }
            walk_boxed_vars(body, st, out);
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            walk_boxed_vars(tpt, st, out);
            walk_boxed_vars(rhs, st, out);
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                walk_boxed_vars(s, st, out);
            }
            walk_boxed_vars(expr, st, out);
        }
        TreeKind::If { cond, thenp, elsep } => {
            walk_boxed_vars(cond, st, out);
            walk_boxed_vars(thenp, st, out);
            walk_boxed_vars(elsep, st, out);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            walk_boxed_vars(cond, st, out);
            walk_boxed_vars(body, st, out);
        }
        TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
            walk_boxed_vars(fun, st, out);
            for a in args {
                walk_boxed_vars(a, st, out);
            }
        }
        TreeKind::Typed { expr, tpt } => {
            walk_boxed_vars(expr, st, out);
            walk_boxed_vars(tpt, st, out);
        }
        TreeKind::Select { qual, .. } => walk_boxed_vars(qual, st, out),
        TreeKind::Assign { lhs, rhs } => {
            walk_boxed_vars(lhs, st, out);
            walk_boxed_vars(rhs, st, out);
        }
        TreeKind::Match { selector, cases } => {
            walk_boxed_vars(selector, st, out);
            for c in cases {
                walk_boxed_vars(&c.pat, st, out);
                walk_boxed_vars(&c.guard, st, out);
                walk_boxed_vars(&c.body, st, out);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            walk_boxed_vars(block, st, out);
            for c in catches {
                walk_boxed_vars(&c.pat, st, out);
                walk_boxed_vars(&c.body, st, out);
            }
            walk_boxed_vars(finalizer, st, out);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            walk_boxed_vars(expr, st, out);
        }
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                walk_boxed_vars(a, st, out);
            }
        }
        _ => {}
    }
}

fn collect_free(tree: &Tree, bound: &HashSet<SymbolId>, out: &mut Vec<SymbolId>, st: &SymbolTable) {
    match &tree.kind {
        TreeKind::Ident { .. } => {
            if !tree.sym.is_none() && !bound.contains(&tree.sym) {
                let s = st.get(tree.sym);
                if s.kind == SymKind::Term && !out.contains(&tree.sym) {
                    out.push(tree.sym);
                }
            }
        }
        TreeKind::Function { vparams, body } => {
            let mut b = bound.clone();
            for p in vparams {
                if !p.sym.is_none() {
                    b.insert(p.sym);
                }
            }
            collect_free(body, &b, out, st);
        }
        TreeKind::Super { .. } | TreeKind::This { .. } => {}
        TreeKind::Select { qual, .. } => collect_free(qual, bound, out, st),
        TreeKind::UnApply { fun, args } => {
            collect_free(fun, bound, out, st);
            for a in args {
                collect_free(a, bound, out, st);
            }
        }
        TreeKind::Apply { fun, args } => {
            collect_free(fun, bound, out, st);
            for a in args {
                collect_free(a, bound, out, st);
            }
        }
        TreeKind::Block { stats, expr } => {
            let mut b = bound.clone();
            for s in stats {
                if let TreeKind::ValDef { .. } = &s.kind {
                    if !s.sym.is_none() {
                        b.insert(s.sym);
                    }
                }
                collect_free(s, &b, out, st);
            }
            collect_free(expr, &b, out, st);
        }
        TreeKind::If { cond, thenp, elsep } => {
            collect_free(cond, bound, out, st);
            collect_free(thenp, bound, out, st);
            collect_free(elsep, bound, out, st);
        }
        TreeKind::ValDef { rhs, .. } => collect_free(rhs, bound, out, st),
        TreeKind::Assign { lhs, rhs } => {
            collect_free(lhs, bound, out, st);
            collect_free(rhs, bound, out, st);
        }
        TreeKind::Typed { expr, .. } | TreeKind::TypeApply { fun: expr, .. } => {
            collect_free(expr, bound, out, st);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            collect_free(cond, bound, out, st);
            collect_free(body, bound, out, st);
        }
        TreeKind::Match { selector, cases } => {
            collect_free(selector, bound, out, st);
            for c in cases {
                collect_free(&c.pat, bound, out, st);
                collect_free(&c.body, bound, out, st);
                collect_free(&c.guard, bound, out, st);
            }
        }
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                collect_free(a, bound, out, st);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            collect_free(block, bound, out, st);
            for c in catches {
                collect_free(&c.pat, bound, out, st);
                collect_free(&c.body, bound, out, st);
                collect_free(&c.guard, bound, out, st);
            }
            collect_free(finalizer, bound, out, st);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            collect_free(expr, bound, out, st);
        }
        TreeKind::New { tpt } => {
            // Instantiating a class that captures enclosing-method locals reads
            // those locals right here.
            if let Some(cid) = new_class_sym(st, tpt) {
                for c in &st.get(cid).captures {
                    if !bound.contains(c) && !out.contains(c) {
                        out.push(*c);
                    }
                }
            }
            collect_free(tpt, bound, out, st);
        }
        _ => {}
    }
}

/// Class symbol instantiated by `new <tpt>`, if it is known.
fn new_class_sym(st: &SymbolTable, tpt: &Tree) -> Option<SymbolId> {
    if !tpt.sym.is_none() && st.get(tpt.sym).is_class_like() {
        return Some(tpt.sym);
    }
    st.class_sym_of(&tpt.ty)
}

fn is_partial_function_ty(st: &SymbolTable, ty: &Type) -> bool {
    match ty {
        Type::Named { name, .. } if name == "PartialFunction" => true,
        Type::Class { sym, .. } => {
            let s = st.get(*sym);
            s.name == "PartialFunction" && s.jvm_name.contains("PartialFunction")
        }
        _ => false,
    }
}

fn pf_match_cases(body: &Tree) -> Option<&[scala_rs_parser::CaseDef]> {
    match &body.kind {
        TreeKind::Match { cases, .. } => Some(cases),
        TreeKind::Block { expr, .. } => pf_match_cases(expr),
        _ => None,
    }
}

fn emit_partial_function_methods(
    b: &mut ClassBuilder,
    st: &SymbolTable,
    extras: &RefCell<Vec<EmittedClass>>,
    lambda_n: &Cell<u32>,
    source: &str,
    class_sym: SymbolId,
    library_abi: bool,
    orig_class: &str,
    lam_name: &str,
    outer_desc: &str,
    need_outer: bool,
    vparams: &[Tree],
    body: &Tree,
    local_caps: &[SymbolId],
    ret_ty: &Type,
    boxed_vars: &HashSet<SymbolId>,
) {
    let cases: Vec<scala_rs_parser::CaseDef> = pf_match_cases(body).unwrap_or(&[]).to_vec();
    let sel_ty = match &body.kind {
        TreeKind::Match { selector, .. } => selector.ty.clone(),
        _ => vparams.first().map(|p| p.ty.clone()).unwrap_or(Type::Any),
    };
    let vparams = vparams.to_vec();
    let local_caps = local_caps.to_vec();
    let lam_name = lam_name.to_string();
    let outer_desc = outer_desc.to_string();
    let orig_class = orig_class.to_string();
    let ret_ty = ret_ty.clone();

    let cases1 = cases.clone();
    let vparams1 = vparams.clone();
    let caps1 = local_caps.clone();
    let lam1 = lam_name.clone();
    let outer1 = outer_desc.clone();
    let orig1 = orig_class.clone();
    let sel1 = sel_ty.clone();

    b.add_code(ACC_PUBLIC, "isDefinedAt", "(Ljava/lang/Object;)Z", 8, |a| {
        let mut fr = Frame::instance();
        fr.next_slot = 2;
        pf_bind_arg_and_captures(a, &mut fr, st, &lam1, &vparams1, &caps1, boxed_vars);
        let some = a.fresh_label();
        let no = a.fresh_label();
        let sel_sort = jvm_sort(&sel1);
        if let Some(p) = vparams1.first() {
            if let Some((slot, sort)) = fr.get(p.sym) {
                load(a, slot, sort);
                let tmp = fr.alloc_tmp(sel_sort);
                store(a, tmp, sel_sort);
                for c in &cases1 {
                    let fail = a.fresh_label();
                    let outer_storage = (lam1.as_str(), "$outer", outer1.as_str());
                    let outer_ref = if need_outer {
                        Some(outer_storage)
                    } else {
                        None
                    };
                    let inner_ctx = EmitCtx {
                        st,
                        class_sym,
                        class_name: &orig1,
                        ret_ty: Type::Boolean,
                        extras,
                        lambda_n,
                        source,
                        outer: outer_ref,
                        library_abi,
                        method_sym: SymbolId::NONE,
                        boxed_vars,
                    };
                    gen_pattern(a, &mut fr, &inner_ctx, &c.pat, tmp, sel_sort, fail);
                    if !c.guard.is_empty() {
                        gen_expr(a, &mut fr, &inner_ctx, &c.guard);
                        a.ifeq(fail);
                    }
                    a.goto(some);
                    a.mark(fail);
                }
            }
        }
        a.goto(no);
        a.mark(some);
        a.iconst(1);
        a.ireturn();
        a.mark(no);
        a.iconst(0);
        a.ireturn();
    });

    b.add_code(
        ACC_PUBLIC,
        "applyOrElse",
        "(Ljava/lang/Object;Lscala/Function1;)Ljava/lang/Object;",
        8,
        |a| {
            let mut fr = Frame::instance();
            fr.next_slot = 3;
            pf_bind_arg_and_captures(a, &mut fr, st, &lam_name, &vparams, &local_caps, boxed_vars);
            let end = a.fresh_label();
            let sel_sort = jvm_sort(&sel_ty);
            if let Some(p) = vparams.first() {
                if let Some((slot, sort)) = fr.get(p.sym) {
                    load(a, slot, sort);
                    let tmp = fr.alloc_tmp(sel_sort);
                    store(a, tmp, sel_sort);
                    for c in &cases {
                        let fail = a.fresh_label();
                        let outer_storage = (lam_name.as_str(), "$outer", outer_desc.as_str());
                        let outer_ref = if need_outer {
                            Some(outer_storage)
                        } else {
                            None
                        };
                        let inner_ctx = EmitCtx {
                            st,
                            class_sym,
                            class_name: &orig_class,
                            ret_ty: ret_ty.clone(),
                            extras,
                            lambda_n,
                            source,
                            outer: outer_ref,
                            library_abi,
                            method_sym: SymbolId::NONE,
                            boxed_vars,
                        };
                        gen_pattern(a, &mut fr, &inner_ctx, &c.pat, tmp, sel_sort, fail);
                        if !c.guard.is_empty() {
                            gen_expr(a, &mut fr, &inner_ctx, &c.guard);
                            a.ifeq(fail);
                        }
                        if is_unit_like(&ret_ty) {
                            gen_stat(a, &mut fr, &inner_ctx, &c.body);
                            emit_box(a, &Type::Unit);
                        } else {
                            gen_expr(a, &mut fr, &inner_ctx, &c.body);
                            emit_box(a, &ret_ty);
                        }
                        a.goto(end);
                        a.mark(fail);
                    }
                }
            }
            a.aload(2);
            a.aload(1);
            a.invokeinterface(
                "scala/Function1",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            );
            a.mark(end);
            a.areturn();
        },
    );
}

fn pf_bind_arg_and_captures(
    a: &mut Assembler,
    fr: &mut Frame,
    st: &SymbolTable,
    lam_name: &str,
    vparams: &[Tree],
    local_caps: &[SymbolId],
    boxed: &HashSet<SymbolId>,
) {
    if let Some(p) = vparams.first() {
        a.aload(1);
        if is_jvm_primitive(&p.ty) || matches!(p.ty, Type::String) {
            emit_unbox(a, &p.ty);
        } else if let Type::Class { sym, .. } = &p.ty {
            let n = class_internal(st, *sym);
            if !n.is_empty() && n != "java/lang/Object" {
                a.checkcast(&n);
            }
        } else {
            emit_unbox(a, &p.ty);
        }
        let sort = jvm_sort(&p.ty);
        let slot = fr.alloc(p.sym, sort);
        store(a, slot, sort);
    }
    for (i, id) in local_caps.iter().enumerate() {
        let ty = st.get(*id).ty.clone();
        a.aload(0);
        a.getfield(lam_name, &format!("$captured${i}"), "Ljava/lang/Object;");
        if boxed.contains(id) {
            a.checkcast(runtime_ref_class(&ty));
            let slot = fr.alloc(*id, JvmSort::Ref);
            store(a, slot, JvmSort::Ref);
        } else {
            emit_from_erased_object(a, st, &ty);
            let sort = jvm_sort(&ty);
            let slot = fr.alloc(*id, sort);
            store(a, slot, sort);
        }
    }
}

fn gen_function(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    let (vparams, body) = match &tree.kind {
        TreeKind::Function { vparams, body } => (vparams, body),
        _ => return,
    };
    let n = ctx.lambda_n.get();
    ctx.lambda_n.set(n + 1);
    let lam_name = format!("{}$$anonfun${}", ctx.class_name.replace('/', "$"), n);
    let arity = vparams.len();
    let is_pf = is_partial_function_ty(ctx.st, &tree.ty);
    let sam = ctx.st.sam_sig(&tree.ty);
    let iface = if is_pf {
        "scala/PartialFunction".to_string()
    } else if let Some(sam) = &sam {
        class_internal(ctx.st, sam.class)
    } else {
        format!("scala/Function{arity}")
    };

    let mut bound = HashSet::new();
    for p in vparams {
        if !p.sym.is_none() {
            bound.insert(p.sym);
        }
    }
    let mut free = Vec::new();
    collect_free(body, &bound, &mut free, ctx.st);

    let mut local_caps = Vec::new();
    let mut need_outer = false;
    for id in &free {
        if frame.get(*id).is_some() {
            local_caps.push(*id);
        } else {
            need_outer = true;
        }
    }
    if tree_contains_return(body) {
        need_outer = true;
    }

    // Create instance: new, dup, load captures, invokespecial
    asm.new_obj(&lam_name);
    asm.dup();
    let mut ctor_desc = String::from("(");
    if need_outer {
        load_this(asm, ctx);
        ctor_desc.push_str(&format!("L{};", ctx.class_name));
    }
    for id in &local_caps {
        let (slot, sort) = frame.get(*id).unwrap();
        let ty = ctx.st.get(*id).ty.clone();
        if is_boxed_var(ctx, *id) {
            // Capture the IntRef/ObjectRef itself, not the elem.
            load(asm, slot, JvmSort::Ref);
        } else {
            load(asm, slot, sort);
            if is_jvm_primitive(&ty) {
                emit_box(asm, &ty);
            }
        }
        ctor_desc.push_str("Ljava/lang/Object;");
    }
    ctor_desc.push_str(")V");
    asm.invokespecial(&lam_name, "<init>", &ctor_desc);

    // Emit the lambda class
    let mut b = ClassBuilder::new(lam_name.clone(), ctx.source);
    b.access = ACC_PUBLIC | ACC_SUPER | ACC_SYNTHETIC | ACC_FINAL;
    b.interfaces.push(iface);
    if need_outer {
        b.fields.push(Field {
            access: ACC_PUBLIC,
            name: "$outer".into(),
            desc: format!("L{};", ctx.class_name),
        });
    }
    for (i, _) in local_caps.iter().enumerate() {
        b.fields.push(Field {
            access: ACC_PUBLIC,
            name: format!("$captured${i}"),
            desc: "Ljava/lang/Object;".into(),
        });
    }
    let cap_n = local_caps.len();
    let class_name_owned = ctx.class_name.to_string();
    let need_outer_c = need_outer;
    b.add_code(
        ACC_PUBLIC,
        "<init>",
        &ctor_desc,
        1 + 1 + cap_n as u16,
        |a| {
            a.aload(0);
            a.invokespecial("java/lang/Object", "<init>", "()V");
            let mut slot = 1u16;
            if need_outer_c {
                a.aload(0);
                a.aload(slot);
                a.putfield(&lam_name, "$outer", &format!("L{class_name_owned};"));
                slot += 1;
            }
            for i in 0..cap_n {
                a.aload(0);
                a.aload(slot);
                a.putfield(&lam_name, &format!("$captured${i}"), "Ljava/lang/Object;");
                slot += 1;
            }
            a.vreturn();
        },
    );

    let mut apply_desc = String::from("(");
    for _ in 0..arity {
        apply_desc.push_str("Ljava/lang/Object;");
    }
    apply_desc.push_str(")Ljava/lang/Object;");
    let sam_emit = sam.as_ref().map(|s| {
        (
            s.name.clone(),
            jvm_method_desc(ctx.st, &s.raw_param_tys, &s.raw_ret_ty),
            s.raw_ret_ty.clone(),
        )
    });
    let (meth_name, meth_desc) = if let Some((n, d, _)) = &sam_emit {
        (n.as_str(), d.as_str())
    } else {
        ("apply", apply_desc.as_str())
    };

    let st = ctx.st;
    let extras = ctx.extras;
    let lambda_n = ctx.lambda_n;
    let source = ctx.source;
    let class_sym = ctx.class_sym;
    let library_abi = ctx.library_abi;
    let boxed = ctx.boxed_vars;
    let orig_class = ctx.class_name.to_string();
    let lam_name2 = lam_name.clone();
    let outer_desc = format!("L{orig_class};");
    let vparams = vparams.clone();
    let body = body.clone();
    let local_caps = local_caps.clone();
    let vparams_pf = vparams.clone();
    let body_pf = body.clone();
    let local_caps_pf = local_caps.clone();
    let ret_ty = if is_pf {
        body.ty.clone()
    } else if let Some(sam) = &sam {
        sam.ret_ty.clone()
    } else {
        match &tree.ty {
            Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        }
    };
    let ret_ty_pf = ret_ty.clone();
    let sam_ret = sam_emit.as_ref().map(|(_, _, r)| r.clone());
    let meth_name_owned = meth_name.to_string();
    let meth_desc_owned = meth_desc.to_string();

    b.add_code(ACC_PUBLIC, &meth_name_owned, &meth_desc_owned, 8, |a| {
        let mut fr = Frame::instance();
        fr.next_slot = 1 + arity as u16;
        // apply args occupy slots 1..arity as Object; remap param symbols after unbox
        for (i, p) in vparams.iter().enumerate() {
            let obj_slot = 1 + i as u16;
            a.aload(obj_slot);
            if is_jvm_primitive(&p.ty) || matches!(p.ty, Type::String) {
                emit_unbox(a, &p.ty);
            } else if let Type::Class { sym, .. } = &p.ty {
                let n = class_internal(st, *sym);
                if !n.is_empty() && n != "java/lang/Object" {
                    a.checkcast(&n);
                }
            } else if matches!(p.ty, Type::Tuple(_)) {
                a.checkcast("scala/Tuple2");
            } else {
                emit_unbox(a, &p.ty);
            }
            let sort = jvm_sort(&p.ty);
            let slot = fr.alloc(p.sym, sort);
            store(a, slot, sort);
        }
        for (i, id) in local_caps.iter().enumerate() {
            let ty = st.get(*id).ty.clone();
            a.aload(0);
            a.getfield(&lam_name2, &format!("$captured${i}"), "Ljava/lang/Object;");
            if boxed.contains(id) {
                a.checkcast(runtime_ref_class(&ty));
                let slot = fr.alloc(*id, JvmSort::Ref);
                store(a, slot, JvmSort::Ref);
            } else {
                emit_from_erased_object(a, st, &ty);
                let sort = jvm_sort(&ty);
                let slot = fr.alloc(*id, sort);
                store(a, slot, sort);
            }
        }
        let outer_storage;
        let outer_ref = if need_outer {
            outer_storage = (lam_name2.as_str(), "$outer", outer_desc.as_str());
            Some(outer_storage)
        } else {
            None
        };
        let inner_ctx = EmitCtx {
            st,
            class_sym,
            class_name: &orig_class,
            ret_ty: ret_ty.clone(),
            extras,
            lambda_n,
            source,
            outer: outer_ref,
            library_abi,
            method_sym: SymbolId::NONE,
            boxed_vars: boxed,
        };
        gen_expr(a, &mut fr, &inner_ctx, &body);
        if matches!(body.ty, Type::Nothing) {
            // `throw` already emits athrow. A following areturn would be an
            // empty-stack stackmap target (`tryBreakable { throw e }`).
        } else if let Some(raw_ret) = &sam_ret {
            if is_unit_like(raw_ret) {
                pop_if_value(a, &body.ty);
                a.vreturn();
            } else if is_jvm_primitive(raw_ret) {
                emit_return(a, raw_ret);
            } else {
                if is_jvm_primitive(&ret_ty) && !is_unit_like(&ret_ty) {
                    emit_box(a, &ret_ty);
                }
                a.areturn();
            }
        } else if is_unit_like(&ret_ty) {
            pop_if_value(a, &body.ty);
            emit_box(a, &Type::Unit);
            a.areturn();
        } else {
            emit_box(a, &ret_ty);
            a.areturn();
        }
    });
    if is_pf {
        emit_partial_function_methods(
            &mut b,
            st,
            extras,
            lambda_n,
            source,
            class_sym,
            library_abi,
            &orig_class,
            &lam_name2,
            &outer_desc,
            need_outer,
            &vparams_pf,
            &body_pf,
            &local_caps_pf,
            &ret_ty_pf,
            ctx.boxed_vars,
        );
    }
    ctx.extras.borrow_mut().push(b.finish());
}

fn gen_println(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    newline: bool,
) {
    asm.getstatic("java/lang/System", "out", "Ljava/io/PrintStream;");
    let name = if newline { "println" } else { "print" };
    if args.is_empty() {
        asm.invokevirtual("java/io/PrintStream", name, "()V");
        return;
    }
    let arg = &args[0];
    match &arg.ty.widen_constant() {
        Type::Unit | Type::NoType => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "()V");
        }
        Type::Int | Type::Byte | Type::Short => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(I)V");
        }
        Type::Long => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(J)V");
        }
        Type::Double => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(D)V");
        }
        Type::Boolean => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(Z)V");
        }
        Type::String => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(Ljava/lang/String;)V");
        }
        Type::Char => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(C)V");
        }
        Type::Float => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(F)V");
        }
        _ => {
            gen_expr(asm, frame, ctx, arg);
            if is_jvm_primitive(&arg.ty) && !matches!(arg.ty, Type::Unit | Type::NoType) {
                emit_box(asm, &arg.ty);
            }
            asm.invokevirtual("java/io/PrintStream", name, "(Ljava/lang/Object;)V");
        }
    }
}

fn emit_int_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.iadd(),
        "-" => asm.isub(),
        "*" => asm.imul(),
        "/" => asm.idiv(),
        "%" => asm.irem(),
        "&" => asm.iand(),
        "|" => asm.ior(),
        "^" => asm.ixor(),
        "<<" => asm.ishl(),
        ">>" => asm.ishr(),
        ">>>" => asm.iushr(),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => emit_int_cmp(asm, op),
        _ => {}
    }
}

fn emit_int_cmp(asm: &mut Assembler, op: &str) {
    let t = asm.fresh_label();
    let e = asm.fresh_label();
    match op {
        "==" => asm.if_icmpeq(t),
        "!=" => asm.if_icmpne(t),
        "<" => asm.if_icmplt(t),
        "<=" => asm.if_icmple(t),
        ">" => asm.if_icmpgt(t),
        ">=" => asm.if_icmpge(t),
        _ => asm.if_icmpeq(t),
    }
    asm.iconst(0);
    asm.goto(e);
    asm.mark(t);
    asm.iconst(1);
    asm.mark(e);
}

fn emit_ref_eq(asm: &mut Assembler, eq: bool) {
    let t = asm.fresh_label();
    let e = asm.fresh_label();
    if eq {
        asm.if_acmpeq(t);
    } else {
        asm.if_acmpne(t);
    }
    asm.iconst(0);
    asm.goto(e);
    asm.mark(t);
    asm.iconst(1);
    asm.mark(e);
}

fn gen_eq_ne(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    eq: bool,
) {
    gen_receiver(asm, frame, ctx, fun);
    if let Some(arg) = args.first() {
        gen_expr(asm, frame, ctx, arg);
        if is_jvm_primitive(&arg.ty) && !matches!(arg.ty, Type::Unit | Type::NoType) {
            emit_box(asm, &arg.ty);
        }
    } else {
        asm.aconst_null();
    }
    emit_ref_eq(asm, eq);
}

fn gen_any_eq(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    eq: bool,
) {
    let recv_ty = match &fun.kind {
        TreeKind::Select { qual, .. } => qual.ty.clone(),
        _ => Type::AnyRef,
    };
    gen_receiver(asm, frame, ctx, fun);
    if is_jvm_primitive(&recv_ty) && !is_unit_like(&recv_ty) {
        emit_box(asm, &recv_ty);
    }
    if let Some(arg) = args.first() {
        gen_expr(asm, frame, ctx, arg);
        if is_jvm_primitive(&arg.ty) && !is_unit_like(&arg.ty) {
            emit_box(asm, &arg.ty);
        }
    } else {
        asm.aconst_null();
    }
    if ctx.library_abi {
        asm.invokestatic(
            "scala/runtime/BoxesRunTime",
            "equals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        );
    } else {
        asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
    }
    if !eq {
        asm.iconst(1);
        asm.ixor();
    }
}

fn gen_synchronized(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    fun: &Tree,
    args: &[Tree],
    result_ty: &Type,
) {
    gen_receiver(asm, frame, ctx, fun);
    let lock = frame.alloc_tmp(JvmSort::Ref);
    store(asm, lock, JvmSort::Ref);
    let sort = jvm_sort(result_ty);
    let result = if sort != JvmSort::Void {
        Some(frame.alloc_tmp(sort))
    } else {
        None
    };
    // Initialize the result local before the try so the exception handler
    // stack map does not claim a live integer that the body never stored.
    if let Some(r) = result {
        push_default(asm, result_ty);
        store(asm, r, sort);
    }
    load(asm, lock, JvmSort::Ref);
    asm.monitorenter();
    let try_s = asm.fresh_label();
    asm.mark(try_s);
    if let Some(body) = args.first() {
        let produced_ty = if let TreeKind::Function { body: inner, .. } = &body.kind {
            gen_expr(asm, frame, ctx, inner);
            inner.ty.clone()
        } else {
            gen_expr(asm, frame, ctx, body);
            if matches!(&body.ty, Type::Function { .. }) {
                asm.invokeinterface("scala/Function0", "apply", "()Ljava/lang/Object;");
            }
            body.ty.clone()
        };
        match sort {
            JvmSort::Ref => {
                if is_jvm_primitive(&produced_ty)
                    && !matches!(produced_ty, Type::Unit | Type::NoType)
                {
                    emit_box(asm, &produced_ty);
                } else if is_unit_like(&produced_ty) {
                    // Unit body: nothing (or popped) — leave a boxed null.
                    push_default(asm, result_ty);
                } else if matches!(result_ty, Type::String) && !matches!(produced_ty, Type::String)
                {
                    asm.checkcast("java/lang/String");
                }
            }
            JvmSort::Void => {
                pop_if_value(asm, &produced_ty);
            }
            _ => {
                if matches!(&produced_ty, Type::Function { .. }) {
                    emit_unbox(asm, result_ty);
                }
            }
        }
    } else {
        push_default(asm, result_ty);
    }
    if let Some(r) = result {
        store(asm, r, sort);
    }
    load(asm, lock, JvmSort::Ref);
    asm.monitorexit();
    let try_e = asm.fresh_label();
    asm.mark(try_e);
    let after = asm.fresh_label();
    asm.goto(after);
    let handler = asm.fresh_label();
    asm.mark(handler);
    asm.enter_handler();
    let ex = frame.alloc_tmp(JvmSort::Ref);
    asm.astore(ex);
    load(asm, lock, JvmSort::Ref);
    asm.monitorexit();
    asm.aload(ex);
    asm.athrow();
    asm.exception(try_s, try_e, handler, None);
    asm.mark(after);
    if let Some(r) = result {
        load(asm, r, sort);
    }
}

fn emit_long_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.ladd(),
        "-" => asm.lsub(),
        "*" => asm.lmul(),
        "/" => asm.ldiv(),
        _ => {}
    }
}

fn emit_double_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.dadd(),
        "-" => asm.dsub(),
        "*" => asm.dmul(),
        "/" => asm.ddiv(),
        _ => {}
    }
}

fn gen_bool_and(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: Option<&Tree>,
) {
    gen_expr(asm, frame, ctx, left);
    let skip = asm.fresh_label();
    asm.dup();
    asm.ifeq(skip);
    asm.pop();
    if let Some(r) = right {
        gen_expr(asm, frame, ctx, r);
    } else {
        asm.iconst(0);
    }
    asm.mark(skip);
}

fn gen_bool_or(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: Option<&Tree>,
) {
    gen_expr(asm, frame, ctx, left);
    let skip = asm.fresh_label();
    asm.dup();
    asm.ifne(skip);
    asm.pop();
    if let Some(r) = right {
        gen_expr(asm, frame, ctx, r);
    } else {
        asm.iconst(1);
    }
    asm.mark(skip);
}

fn gen_string_concat(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: &Tree,
) {
    asm.new_obj("java/lang/StringBuilder");
    asm.dup();
    asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
    gen_sb_append(asm, frame, ctx, left);
    gen_sb_append(asm, frame, ctx, right);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
}

fn gen_interpolated(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    parts: &[String],
    args: &[Tree],
) {
    asm.new_obj("java/lang/StringBuilder");
    asm.dup();
    asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
    for i in 0..args.len() {
        if i < parts.len() {
            sb_append_string(asm, &parts[i]);
        }
        gen_sb_append(asm, frame, ctx, &args[i]);
    }
    if parts.len() > args.len() {
        sb_append_string(asm, &parts[args.len()]);
    }
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
}

fn gen_f_interpolated(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    parts: &[String],
    args: &[Tree],
) {
    let format = match scala_rs_parser::finterp::assemble_f(parts, args.len()) {
        Ok((fmt, _)) => fmt,
        Err(_) => {
            // Typer already diagnosed unsupported bits; keep the classfile
            // well-formed rather than inventing a successful format.
            asm.ldc_string("");
            return;
        }
    };
    asm.ldc_string(&format);
    asm.iconst(args.len() as i32);
    asm.anewarray("java/lang/Object");
    for (i, a) in args.iter().enumerate() {
        asm.dup();
        asm.iconst(i as i32);
        gen_expr(asm, frame, ctx, a);
        if is_jvm_primitive(&a.ty) {
            emit_box(asm, &a.ty);
        } else if matches!(a.ty, Type::Unit | Type::NoType) {
            // boxed already as null from emit_box
            emit_box(asm, &a.ty);
        }
        asm.aastore();
    }
    asm.invokestatic(
        "java/lang/String",
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
    );
}

fn sb_append_string(asm: &mut Assembler, s: &str) {
    if s.is_empty() {
        return;
    }
    asm.ldc_string(s);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
}

fn gen_sb_append(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, value: &Tree) {
    let desc = match &value.ty {
        Type::Unit | Type::NoType => {
            asm.ldc_string("()");
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
        }
        Type::Int | Type::Byte | Type::Short => {
            gen_expr(asm, frame, ctx, value);
            "(I)Ljava/lang/StringBuilder;"
        }
        Type::Long => {
            gen_expr(asm, frame, ctx, value);
            "(J)Ljava/lang/StringBuilder;"
        }
        Type::Double => {
            gen_expr(asm, frame, ctx, value);
            "(D)Ljava/lang/StringBuilder;"
        }
        Type::Float => {
            gen_expr(asm, frame, ctx, value);
            "(F)Ljava/lang/StringBuilder;"
        }
        Type::Boolean => {
            gen_expr(asm, frame, ctx, value);
            "(Z)Ljava/lang/StringBuilder;"
        }
        Type::Char => {
            gen_expr(asm, frame, ctx, value);
            "(C)Ljava/lang/StringBuilder;"
        }
        Type::String => {
            gen_expr(asm, frame, ctx, value);
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
        }
        _ => {
            gen_expr(asm, frame, ctx, value);
            "(Ljava/lang/Object;)Ljava/lang/StringBuilder;"
        }
    };
    asm.invokevirtual("java/lang/StringBuilder", "append", desc);
}

fn gen_try(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    block: &Tree,
    catches: &[scala_rs_parser::CaseDef],
    finalizer: &Tree,
    result_ty: &Type,
) {
    let unit = is_unit_like(result_ty);
    let has_finally = !finalizer.is_empty();
    let start = asm.fresh_label();
    let end_try = asm.fresh_label();
    let handler = asm.fresh_label();
    let after = asm.fresh_label();
    let sel_sort = if unit {
        JvmSort::Void
    } else {
        jvm_sort(result_ty)
    };
    let result_slot = if unit {
        None
    } else {
        Some(frame.alloc_tmp(sel_sort))
    };
    // Initialize the result local before the try so the exception handler
    // stack map does not claim a live value that the body never stored.
    if let Some(slot) = result_slot {
        push_default(asm, result_ty);
        store(asm, slot, sel_sort);
    }
    let exn_slot = frame.alloc_tmp(JvmSort::Ref);

    asm.mark(start);
    if unit {
        gen_stat(asm, frame, ctx, block);
    } else {
        gen_expr(asm, frame, ctx, block);
        store(asm, result_slot.unwrap(), sel_sort);
    }
    asm.mark(end_try);
    if has_finally {
        gen_stat(asm, frame, ctx, finalizer);
    }
    asm.goto(after);

    asm.mark(handler);
    asm.enter_handler();
    store(asm, exn_slot, JvmSort::Ref);
    let catch_rethrow = if has_finally && !catches.is_empty() {
        Some(asm.fresh_label())
    } else {
        None
    };
    for c in catches {
        let fail = asm.fresh_label();
        gen_pattern(asm, frame, ctx, &c.pat, exn_slot, JvmSort::Ref, fail);
        if !c.guard.is_empty() {
            gen_expr(asm, frame, ctx, &c.guard);
            asm.ifeq(fail);
        }
        let catch_start = asm.fresh_label();
        let catch_end = asm.fresh_label();
        asm.mark(catch_start);
        if unit {
            gen_stat(asm, frame, ctx, &c.body);
        } else if is_unit_like(&c.body.ty) {
            // `try { resources(...) /* Int */ } catch { println }` — nsc LUBs
            // to Any in statement position. We keep the try's type and fill
            // a default so the catch does not istore from an empty stack.
            gen_stat(asm, frame, ctx, &c.body);
            push_default(asm, result_ty);
            if let Some(slot) = result_slot {
                store(asm, slot, sel_sort);
            }
        } else {
            gen_expr(asm, frame, ctx, &c.body);
            if let Some(slot) = result_slot {
                store(asm, slot, sel_sort);
            }
        }
        asm.mark(catch_end);
        if has_finally {
            gen_stat(asm, frame, ctx, finalizer);
        }
        asm.goto(after);
        if let Some(rethrow) = catch_rethrow {
            asm.exception(catch_start, catch_end, rethrow, Some("java/lang/Throwable"));
        }
        asm.mark(fail);
    }
    if has_finally {
        gen_stat(asm, frame, ctx, finalizer);
    }
    load(asm, exn_slot, JvmSort::Ref);
    asm.athrow();

    if let Some(rethrow) = catch_rethrow {
        asm.mark(rethrow);
        asm.enter_handler();
        store(asm, exn_slot, JvmSort::Ref);
        gen_stat(asm, frame, ctx, finalizer);
        load(asm, exn_slot, JvmSort::Ref);
        asm.athrow();
    }

    asm.mark(after);
    if let Some(slot) = result_slot {
        load(asm, slot, sel_sort);
    }
    asm.exception(start, end_try, handler, Some("java/lang/Throwable"));
}

fn gen_match(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    selector: &Tree,
    cases: &[scala_rs_parser::CaseDef],
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, selector);
    let sel_sort = jvm_sort(&selector.ty);
    let tmp = frame.alloc_tmp(sel_sort);
    store(asm, tmp, sel_sort);
    if gen_int_switch(asm, frame, ctx, selector, cases, result_ty, tmp, sel_sort) {
        return;
    }
    let end = asm.fresh_label();
    for c in cases {
        let fail = asm.fresh_label();
        gen_pattern(asm, frame, ctx, &c.pat, tmp, sel_sort, fail);
        if !c.guard.is_empty() {
            gen_expr(asm, frame, ctx, &c.guard);
            asm.ifeq(fail);
        }
        if is_unit_like(result_ty) {
            gen_stat(asm, frame, ctx, &c.body);
        } else {
            gen_expr(asm, frame, ctx, &c.body);
        }
        asm.goto(end);
        asm.mark(fail);
    }
    throw_runtime(asm, "match error");
    asm.mark(end);
}

enum SwitchPat {
    Key(i32),
    Default,
}

fn switch_pat_key(pat: &Tree) -> Option<SwitchPat> {
    match &pat.kind {
        TreeKind::Literal { lit: Lit::Int(n) } => Some(SwitchPat::Key(*n)),
        TreeKind::Literal { lit: Lit::Char(c) } => Some(SwitchPat::Key(*c as i32)),
        TreeKind::Wildcard | TreeKind::Empty => Some(SwitchPat::Default),
        TreeKind::Ident { name } => {
            let is_varid = name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_');
            if is_varid {
                Some(SwitchPat::Default)
            } else {
                None
            }
        }
        TreeKind::Bind { body, .. } => switch_pat_key(body),
        TreeKind::Typed { expr, .. } => switch_pat_key(expr),
        _ => None,
    }
}

fn peel_type_annot<'a>(ty: &'a Type) -> &'a Type {
    match ty {
        Type::Annotated { tpe, .. } => peel_type_annot(tpe),
        t => t,
    }
}

fn gen_int_switch(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    selector: &Tree,
    cases: &[scala_rs_parser::CaseDef],
    result_ty: &Type,
    tmp: u16,
    sel_sort: JvmSort,
) -> bool {
    if sel_sort != JvmSort::Int {
        return false;
    }
    let core = peel_type_annot(&selector.ty);
    if !matches!(core, Type::Int | Type::Char) {
        return false;
    }
    let mut keys: Vec<(i32, usize)> = Vec::new();
    let mut default_idx: Option<usize> = None;
    for (i, c) in cases.iter().enumerate() {
        if !c.guard.is_empty() {
            return false;
        }
        match switch_pat_key(&c.pat) {
            Some(SwitchPat::Key(k)) => {
                if keys.iter().any(|(ek, _)| *ek == k) {
                    return false;
                }
                keys.push((k, i));
            }
            Some(SwitchPat::Default) => {
                if default_idx.is_some() {
                    return false;
                }
                default_idx = Some(i);
            }
            None => return false,
        }
    }
    if keys.is_empty() {
        return false;
    }
    let mut case_labs: Vec<crate::code::Label> = Vec::new();
    for _ in cases {
        case_labs.push(asm.fresh_label());
    }
    let miss = asm.fresh_label();
    let end = asm.fresh_label();
    let def_lab = default_idx.map(|i| case_labs[i]).unwrap_or(miss);
    load(asm, tmp, sel_sort);
    let lo = keys.iter().map(|(k, _)| *k).min().unwrap();
    let hi = keys.iter().map(|(k, _)| *k).max().unwrap();
    let n = keys.len() as i64;
    let space = hi as i64 - lo as i64 + 1;
    if space <= n * 2 && space <= 4096 {
        let mut table: Vec<crate::code::Label> = Vec::new();
        for k in lo..=hi {
            let lab = keys
                .iter()
                .find(|(ek, _)| *ek == k)
                .map(|(_, i)| case_labs[*i])
                .unwrap_or(def_lab);
            table.push(lab);
        }
        asm.tableswitch(def_lab, lo, hi, &table);
    } else {
        let mut pairs: Vec<(i32, crate::code::Label)> =
            keys.iter().map(|(k, i)| (*k, case_labs[*i])).collect();
        pairs.sort_by_key(|(k, _)| *k);
        asm.lookupswitch(def_lab, &pairs);
    }
    for (i, c) in cases.iter().enumerate() {
        asm.mark(case_labs[i]);
        if let TreeKind::Ident { .. } | TreeKind::Bind { .. } = &c.pat.kind {
            if matches!(switch_pat_key(&c.pat), Some(SwitchPat::Default)) {
                gen_pattern(asm, frame, ctx, &c.pat, tmp, sel_sort, miss);
            }
        }
        if is_unit_like(result_ty) {
            gen_stat(asm, frame, ctx, &c.body);
        } else {
            gen_expr(asm, frame, ctx, &c.body);
        }
        asm.goto(end);
    }
    if default_idx.is_none() {
        asm.mark(miss);
        throw_runtime(asm, "match error");
    }
    asm.mark(end);
    true
}

fn gen_unapply_pattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    fun: &Tree,
    args: &[Tree],
    tmp: u16,
    sel_sort: JvmSort,
    fail: crate::code::Label,
) {
    let uid = if pat.sym.is_none() { fun.sym } else { pat.sym };
    let ret_bool = !uid.is_none() && matches!(ctx.st.get(uid).ty.result(), Type::Boolean);
    let param0 = if uid.is_none() {
        None
    } else {
        match &ctx.st.get(uid).ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|ps| ps.first()).cloned(),
            Type::Function { params, .. } => params.first().cloned(),
            _ => None,
        }
    };
    gen_receiver(asm, frame, ctx, fun);
    load(asm, tmp, sel_sort);
    if let Some(p) = &param0 {
        if !is_jvm_primitive(p) && sel_sort != JvmSort::Ref && sel_sort != JvmSort::Void {
            let pty = match sel_sort {
                JvmSort::Int => Type::Int,
                JvmSort::Long => Type::Long,
                JvmSort::Double => Type::Double,
                JvmSort::Float => Type::Float,
                _ => Type::Any,
            };
            emit_box(asm, &pty);
        }
    }
    if uid.is_none() {
        asm.pop();
        asm.pop();
        throw_runtime(asm, "unresolved unapply");
        return;
    }
    invoke_method(asm, ctx, uid, None);
    if ret_bool {
        asm.ifeq(fail);
        return;
    }
    let is_seq = ctx.st.get(uid).name == "unapplySeq";
    if is_seq && ctx.library_abi && is_list_unapply_seq(ctx.st, uid) {
        // scala-library `List.unapplySeq` is identity on SeqOps, not Option.
        gen_unapply_seq_bind(asm, frame, ctx, args, fail);
        return;
    }
    asm.dup();
    asm.invokevirtual("scala/Option", "isEmpty", "()Z");
    let nonempty = asm.fresh_label();
    asm.ifeq(nonempty);
    asm.pop();
    asm.goto(fail);
    asm.mark(nonempty);
    asm.invokevirtual("scala/Option", "get", "()Ljava/lang/Object;");
    let is_seq = ctx.st.get(uid).name == "unapplySeq";
    if is_seq {
        gen_unapply_seq_bind(asm, frame, ctx, args, fail);
        return;
    }
    if args.len() <= 1 {
        if let Some(a) = args.first() {
            if is_jvm_primitive(&a.ty) {
                emit_unbox(asm, &a.ty);
            } else if matches!(a.ty, Type::String) {
                asm.checkcast("java/lang/String");
            }
            bind_subpattern(asm, frame, ctx, a, fail);
        } else {
            asm.pop();
        }
    } else {
        asm.checkcast("scala/Tuple2");
        for (i, a) in args.iter().enumerate() {
            let fname = if i == 0 { "_1" } else { "_2" };
            asm.dup();
            asm.getfield("scala/Tuple2", fname, "Ljava/lang/Object;");
            if is_jvm_primitive(&a.ty) {
                emit_unbox(asm, &a.ty);
            }
            bind_subpattern(asm, frame, ctx, a, fail);
        }
        asm.pop();
    }
}

fn gen_unapply_seq_bind(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    fail: crate::code::Label,
) {
    asm.checkcast("scala/collection/immutable/List");
    let list_slot = frame.alloc_tmp(JvmSort::Ref);
    store(asm, list_slot, JvmSort::Ref);
    let mut saw_star = false;
    for a in args {
        if is_star_pat(a) {
            load(asm, list_slot, JvmSort::Ref);
            bind_subpattern(asm, frame, ctx, a, fail);
            saw_star = true;
            break;
        }
        load(asm, list_slot, JvmSort::Ref);
        asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
        asm.ifne(fail);
        load(asm, list_slot, JvmSort::Ref);
        emit_list_head(asm, ctx);
        if is_jvm_primitive(&a.ty) {
            emit_unbox(asm, &a.ty);
        } else if matches!(a.ty, Type::String) {
            asm.checkcast("java/lang/String");
        }
        bind_subpattern(asm, frame, ctx, a, fail);
        load(asm, list_slot, JvmSort::Ref);
        emit_list_tail(asm, ctx);
        store(asm, list_slot, JvmSort::Ref);
    }
    if !saw_star {
        load(asm, list_slot, JvmSort::Ref);
        asm.invokevirtual("scala/collection/immutable/List", "isEmpty", "()Z");
        asm.ifeq(fail);
    }
}

fn emit_list_head(asm: &mut Assembler, ctx: &EmitCtx) {
    if ctx.library_abi {
        // `head` is on LinearSeqOps; List itself has no `head()Object` method.
        asm.invokeinterface(
            "scala/collection/LinearSeqOps",
            "head",
            "()Ljava/lang/Object;",
        );
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "head",
            "()Ljava/lang/Object;",
        );
    }
}

fn emit_list_tail(asm: &mut Assembler, ctx: &EmitCtx) {
    if ctx.library_abi {
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "tail",
            "()Lscala/collection/LinearSeq;",
        );
        asm.checkcast("scala/collection/immutable/List");
    } else {
        asm.invokevirtual(
            "scala/collection/immutable/List",
            "tail",
            "()Lscala/collection/immutable/List;",
        );
    }
}

fn gen_pattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    tmp: u16,
    sel_sort: JvmSort,
    fail: crate::code::Label,
) {
    match &pat.kind {
        TreeKind::Wildcard | TreeKind::Empty => {}
        TreeKind::Ident { name } => {
            let is_varid = name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_');
            if is_varid || pat.sym.is_none() || ctx.st.get(pat.sym).kind == SymKind::Term {
                load(asm, tmp, sel_sort);
                let sort = jvm_sort(&pat.ty);
                let slot = if pat.sym.is_none() {
                    frame.alloc_tmp(sort)
                } else if let Some((s, _)) = frame.get(pat.sym) {
                    s
                } else {
                    frame.alloc(pat.sym, sort)
                };
                store(asm, slot, sort);
            } else {
                load(asm, tmp, sel_sort);
                gen_ident(asm, frame, ctx, pat);
                match sel_sort {
                    JvmSort::Int => asm.if_icmpne(fail),
                    JvmSort::Ref => {
                        let ok = asm.fresh_label();
                        // reference equality then equals
                        // stack: tmp, ident — use Object.equals
                        asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
                        asm.ifne(ok);
                        asm.goto(fail);
                        asm.mark(ok);
                    }
                    _ => {
                        pop_if_value(asm, &pat.ty);
                        pop_if_value(asm, &pat.ty);
                    }
                }
            }
        }
        TreeKind::Literal { lit } => {
            load(asm, tmp, sel_sort);
            gen_literal(asm, lit);
            match sel_sort {
                JvmSort::Int => asm.if_icmpne(fail),
                JvmSort::Ref => {
                    asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
                    asm.ifeq(fail);
                }
                _ => {
                    asm.pop();
                    asm.pop();
                }
            }
        }
        TreeKind::Select { .. } => {
            load(asm, tmp, sel_sort);
            gen_expr(asm, frame, ctx, pat);
            match sel_sort {
                JvmSort::Ref => {
                    asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
                    asm.ifeq(fail);
                }
                JvmSort::Int => asm.if_icmpne(fail),
                _ => {
                    pop_if_value(asm, &pat.ty);
                    pop_if_value(asm, &pat.ty);
                    asm.goto(fail);
                }
            }
        }
        TreeKind::Apply { args, .. } => {
            let class_id = if pat.sym.is_none() {
                ctx.st.class_sym_of(&pat.ty).unwrap_or(SymbolId::NONE)
            } else {
                pat.sym
            };
            let jvm = if class_id.is_none() {
                pat.name().unwrap_or("java/lang/Object").to_string()
            } else {
                class_internal(ctx.st, class_id)
            };
            load(asm, tmp, JvmSort::Ref);
            asm.instanceof(&jvm);
            asm.ifeq(fail);
            let fields = if class_id.is_none() {
                Vec::new()
            } else {
                ctx.st.get(class_id).ctor_fields.clone()
            };
            for (i, a) in args.iter().enumerate() {
                if let Some(fid) = fields.get(i) {
                    let fs = ctx.st.get(*fid);
                    let fname = fs.name.clone();
                    let fty = fs.ty.clone();
                    let fdesc = jvm_desc(ctx.st, &fty);
                    load(asm, tmp, JvmSort::Ref);
                    asm.checkcast(&jvm);
                    // Library classes keep the field private; `jvm_name` on the
                    // constructor field names the accessor to call instead.
                    let acc = ctx.st.get(*fid).jvm_name.clone();
                    if acc.is_empty() {
                        asm.getfield(&jvm, &fname, &fdesc);
                    } else {
                        asm.invokevirtual(&jvm, &acc, &format!("(){fdesc}"));
                    }
                    // A field declared as a type parameter erases to Object, so
                    // `case Some(x)` on an `Option[Int]` must unbox before it binds.
                    if fdesc == "Ljava/lang/Object;" {
                        emit_from_erased_object(asm, ctx.st, &a.ty);
                    }
                    bind_subpattern(asm, frame, ctx, a, fail);
                } else {
                    throw_runtime(asm, "pattern arity");
                }
            }
        }
        TreeKind::UnApply { fun, args } => {
            gen_unapply_pattern(asm, frame, ctx, pat, fun, args, tmp, sel_sort, fail);
        }
        TreeKind::Bind { body, .. } => {
            load(asm, tmp, sel_sort);
            let sort = jvm_sort(&pat.ty);
            let slot = if pat.sym.is_none() {
                frame.alloc_tmp(sort)
            } else {
                frame.alloc(pat.sym, sort)
            };
            store(asm, slot, sort);
            gen_pattern(asm, frame, ctx, body, tmp, sel_sort, fail);
        }
        TreeKind::Typed { expr, .. } => {
            let jvm = type_jvm_name(ctx.st, &pat.ty);
            if jvm != "java/lang/Object" {
                load(asm, tmp, JvmSort::Ref);
                asm.instanceof(&jvm);
                asm.ifeq(fail);
            }
            // `case i: Int` / `case s: String` narrows an `Object` scrutinee,
            // so the bound value is unboxed or cast before it is stored.
            let want = jvm_sort(&pat.ty);
            if want != sel_sort || jvm != "java/lang/Object" {
                load(asm, tmp, sel_sort);
                emit_from_erased_object(asm, ctx.st, &pat.ty);
                let narrowed = frame.alloc_tmp(want);
                store(asm, narrowed, want);
                gen_pattern(asm, frame, ctx, expr, narrowed, want, fail);
            } else {
                gen_pattern(asm, frame, ctx, expr, tmp, sel_sort, fail);
            }
        }
        _ => {}
    }
}

fn bind_subpattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    fail: crate::code::Label,
) {
    // field value is on the stack
    match &pat.kind {
        TreeKind::Wildcard | TreeKind::Empty => {
            pop_if_value(asm, &pat.ty);
        }
        // A lowercase identifier binds; `Nil` and other stable ids must be
        // compared, so they fall through to `gen_pattern` below.
        TreeKind::Ident { name }
            if name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_')
                || pat.sym.is_none()
                || ctx.st.get(pat.sym).kind == SymKind::Term =>
        {
            let sort = jvm_sort(&pat.ty);
            let slot = if pat.sym.is_none() {
                frame.alloc_tmp(sort)
            } else if let Some((s, _)) = frame.get(pat.sym) {
                s
            } else {
                frame.alloc(pat.sym, sort)
            };
            store(asm, slot, sort);
        }
        _ => {
            // Nested patterns (`case h :: Nil`, `case Some(Some(x))`) need the
            // full matcher, which reads its value from a local.
            let sort = jvm_sort(&pat.ty);
            let tmp = frame.alloc_tmp(sort);
            store(asm, tmp, sort);
            gen_pattern(asm, frame, ctx, pat, tmp, sort, fail);
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classfile::write_class_file;
    use scala_rs_typer::{has_errors, typecheck_str, typecheck_str_opts};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fresh_dir() -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "scala-rs-backend-{}-{}-{}",
            std::process::id(),
            n,
            nanos
        ));
        std::fs::create_dir_all(&p).expect("temp dir");
        TempDir(p)
    }

    fn java_available() -> bool {
        Command::new("java")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn write_classes(dir: &Path, classes: &[EmittedClass]) {
        for c in classes {
            let mut path = dir.to_path_buf();
            let parts: Vec<&str> = c
                .internal_name
                .split('/')
                .filter(|p| !p.is_empty())
                .collect();
            match parts.split_last() {
                Some((file, dirs)) => {
                    for d in dirs {
                        path.push(d);
                    }
                    path.push(format!("{file}.class"));
                }
                None => path.push(".class"),
            }
            write_class_file(&path, &c.bytes).expect("write class");
        }
    }

    fn compile_src(src: &str) -> Vec<EmittedClass> {
        let (mut tree, mut st, diags) = typecheck_str(src);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        scala_rs_typer::uncurry(&mut tree, &mut st);
        scala_rs_typer::lambda_lift(&mut tree, &mut st);
        scala_rs_typer::erase(&mut tree, &mut st);
        let mut classes = crate::runtime::emit_runtime();
        classes.extend(emit(&tree, &st, "Test.scala"));
        classes
    }

    fn compile_src_library(src: &str) -> Vec<EmittedClass> {
        let (mut tree, mut st, diags) = typecheck_str_opts(
            src,
            &scala_rs_typer::TypecheckOptions {
                fatal_warnings: false,
                library_abi: true,
                classpath: Vec::new(),
                binary_path: Vec::new(),
                language_features: Vec::new(),
            },
        );
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        scala_rs_typer::uncurry(&mut tree, &mut st);
        scala_rs_typer::lambda_lift(&mut tree, &mut st);
        scala_rs_typer::erase(&mut tree, &mut st);
        emit_opts(
            &tree,
            &st,
            "Test.scala",
            EmitOpts {
                library_abi: true,
                ..Default::default()
            },
        )
    }

    fn run_main(src: &str) -> Option<String> {
        if !java_available() {
            return None;
        }
        let classes = compile_src(src);
        assert!(!classes.is_empty(), "no classes emitted");
        let tmp = fresh_dir();
        write_classes(&tmp.0, &classes);
        let output = Command::new("java")
            .arg("-cp")
            .arg(&tmp.0)
            .arg("Main")
            .output()
            .expect("java");
        if !output.status.success() {
            let _ = Command::new("javap")
                .args(["-c", "-p", "-classpath"])
                .arg(&tmp.0)
                .arg("Main")
                .arg("Main$")
                .status();
            panic!(
                "java failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    #[test]
    fn emit_hello_class_names_and_magic() {
        let classes = compile_src(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        );
        let names: Vec<&str> = classes.iter().map(|c| c.internal_name.as_str()).collect();
        assert!(names.contains(&"Main$"), "missing Main$ in {names:?}");
        assert!(names.contains(&"Main"), "missing Main in {names:?}");
        for c in &classes {
            assert!(
                c.bytes.len() >= 4 && c.bytes[0..4] == [0xCA, 0xFE, 0xBA, 0xBE],
                "{} is not a classfile",
                c.internal_name
            );
        }
    }

    #[test]
    fn dense_int_match_emits_tableswitch() {
        let classes = compile_src(
            r#"
import scala.annotation.switch
object Main {
  def dense(n: Int): Int = (n: @switch) match {
    case 0 => 10
    case 1 => 11
    case 2 => 12
    case 3 => 13
    case 4 => 14
  }
  def sparse(n: Int): Int = (n: @switch) match {
    case 0 => 1
    case 100 => 2
    case 200 => 3
  }
  def main(args: Array[String]): Unit = {
    println(dense(2))
    println(sparse(100))
  }
}
"#,
        );
        let main = classes
            .iter()
            .find(|c| c.internal_name == "Main$")
            .expect("Main$");
        assert!(
            main.bytes.contains(&0xaa),
            "dense Int match should emit tableswitch (0xaa)"
        );
        assert!(
            main.bytes.contains(&0xab),
            "sparse Int match should emit lookupswitch (0xab)"
        );
    }

    #[test]
    fn library_abi_does_not_emit_option_or_list() {
        let classes = compile_src_library(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: Nil
    val o = Some(1)
    println(xs)
    println(o)
  }
}
"#,
        );
        for c in &classes {
            assert!(
                !c.internal_name.starts_with("scala/"),
                "library ABI must not emit {} (use scala-library.jar)",
                c.internal_name
            );
        }
        assert!(classes.iter().any(|c| c.internal_name == "Main$"));
    }

    #[test]
    fn hello_world_prints_3() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        ) else {
            return;
        };
        assert!(
            out.contains('3') || out.to_lowercase().contains("hello"),
            "stdout: {out:?}"
        );
    }

    #[test]
    fn factorial_5_prints_120() {
        let Some(out) = run_main(
            r#"
object Main {
  def fact(n: Int): Int =
    if (n <= 1) 1 else n * fact(n - 1)
  def main(args: Array[String]): Unit = println(fact(5))
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("120"), "stdout: {out:?}");
    }

    #[test]
    fn hello_fixture_string() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    println("hello, scala-rs")
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("hello, scala-rs"), "stdout: {out:?}");
    }

    #[test]
    fn arithmetic_fixture() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(1 + 2 * 3)
    println(10 - 4)
    println(20 / 4)
    println(7 % 3)
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('7'), "stdout: {out:?}");
        assert!(out.contains('6'), "stdout: {out:?}");
        assert!(out.contains('5'), "stdout: {out:?}");
        assert!(out.contains('1'), "stdout: {out:?}");
    }

    #[test]
    fn counter_class() {
        let Some(out) = run_main(
            r#"
class Counter(start: Int) {
  var n: Int = start
  def inc(): Unit = { n = n + 1 }
  def get(): Int = n
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new Counter(10)
    c.inc()
    c.inc()
    println(c.get())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("12"), "stdout: {out:?}");
    }

    #[test]
    fn case_class_match() {
        let Some(out) = run_main(
            r#"
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(3, 4)
    println(p match {
      case Point(a, b) => a + b
    })
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('7'), "stdout: {out:?}");
    }

    #[test]
    fn trait_impl() {
        let Some(out) = run_main(
            r#"
trait Greeter {
  def greet(name: String): String
}
class HelloGreeter extends Greeter {
  def greet(name: String): String = "Hello, " + name
}
object Main {
  def main(args: Array[String]): Unit = {
    val g: Greeter = new HelloGreeter()
    println(g.greet("Scala"))
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("Hello, Scala"), "stdout: {out:?}");
    }

    #[test]
    fn while_loop() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    var i: Int = 0
    while (i < 3) {
      i = i + 1
    }
    println(i)
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('3'), "stdout: {out:?}");
    }

    #[test]
    fn string_interpolation() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val name = "world"
    println(s"hello $name")
    val n: Int = 7
    println(f"$n%02d")
    println(raw"a\nb")
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("hello world"), "stdout: {out:?}");
        assert!(out.contains("07"), "stdout: {out:?}");
        assert!(out.contains("a\\nb"), "stdout: {out:?}");
    }

    #[test]
    fn trait_concrete_method() {
        let Some(out) = run_main(
            r#"
trait T {
  def greet(): String = "from trait"
}
class C extends T
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().greet())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("from trait"), "stdout: {out:?}");
    }

    #[test]
    fn trait_linearization_last_mixin_wins() {
        let Some(out) = run_main(
            r#"
trait A {
  def msg: String = "A"
}
trait B {
  def msg: String = "B"
}
class C extends A with B
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out.trim(), "B", "stdout: {out:?}");
    }

    #[test]
    fn try_catch_finally_prints() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    try {
      println("before")
      throw new RuntimeException()
      println("after")
    } catch {
      case _: RuntimeException => println("caught")
    } finally {
      println("finally")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "before\ncaught\nfinally\n", "stdout: {out:?}");
    }

    #[test]
    fn try_finally_success_and_throw() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    try {
      println("ok")
    } finally {
      println("fin-ok")
    }
    try {
      try {
        println("before-throw")
        throw new RuntimeException()
      } finally {
        println("fin-throw")
      }
    } catch {
      case _: RuntimeException => println("outer")
    }
    try {
      try {
        throw new RuntimeException()
      } catch {
        case _: RuntimeException =>
          println("caught")
          throw new RuntimeException()
      } finally {
        println("fin-catch")
      }
    } catch {
      case _: RuntimeException => println("outer2")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(
            out, "ok\nfin-ok\nbefore-throw\nfin-throw\nouter\ncaught\nfin-catch\nouter2\n",
            "stdout: {out:?}"
        );
    }

    #[test]
    fn update_assignment_array_and_user_def() {
        let Some(out) = run_main(
            r#"
class Cell {
  var n: Int = 0
  def update(i: Int, v: Int): Unit = { n = v + i }
  def apply(i: Int): Int = n + i
}
object Main {
  def main(args: Array[String]): Unit = {
    val arr = new Array[Int](2)
    arr(0) = 1
    arr(1) = 2
    println(arr(0))
    arr(1) = 9
    println(arr(1))
    val c = new Cell()
    c(1) = 10
    println(c.n)
    println(c(2))
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "1\n9\n11\n13\n", "stdout: {out:?}");
    }

    #[test]
    fn nested_inner_class() {
        let Some(out) = run_main(
            r#"
class Outer {
  class Inner {
    def hi(): String = "inner"
  }
  def make(): String = new Inner().hi()
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Outer().make())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("inner"), "stdout: {out:?}");
    }

    #[test]
    fn nested_object() {
        let Some(out) = run_main(
            r#"
object Outer {
  object Inner {
    def hi(): String = "nested"
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    println(Outer.Inner.hi())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("nested"), "stdout: {out:?}");
    }

    #[test]
    fn nonlocal_return_from_foreach_lambda() {
        let Some(out) = run_main(
            r#"
object Main {
  def find(xs: List[Int]): Int = {
    xs.foreach((x: Int) => { if (x > 0) return x })
    0
  }
  def nested: Int = {
    def inner: Int = { return 1 }
    inner
  }
  def main(args: Array[String]): Unit = {
    println(find(1 :: 2 :: Nil))
    println(find((-1) :: 3 :: Nil))
    println(nested)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "1\n3\n1\n", "stdout: {out:?}");
    }

    #[test]
    fn super_call_and_qualified_this() {
        let Some(out) = run_main(
            r#"
class Base {
  def greet(): String = "base"
}
trait T {
  def greet(): String = "T"
}
class C extends Base {
  def hi(): String = super.greet() + "!"
}
class D extends T {
  def hi(): String = super.greet() + "!"
}
class Outer {
  val name: String = "outer"
  class Inner {
    def who(): String = Outer.this.name
  }
  def inner(): String = new Inner().who()
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().hi())
    println(new D().hi())
    println(new Outer().inner())
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "base!\nT!\nouter\n", "stdout: {out:?}");
    }

    #[test]
    fn sealed_match_and_unapply() {
        let Some(out) = run_main(
            r#"
sealed trait Color
case class RGB(n: Int) extends Color
case object Black extends Color
object Even {
  def unapply(n: Int): Option[Int] = if (n % 2 == 0) Some(n / 2) else None
}
object Main {
  def show(c: Color): Int = c match {
    case RGB(n) => n
    case Black => 0
  }
  def main(args: Array[String]): Unit = {
    println(show(RGB(3)))
    println(show(Black))
    val x = 10 match {
      case Even(half) => half
      case _ => 0
    }
    println(x)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "3\n0\n5\n", "stdout: {out:?}");
    }

    #[test]
    fn value_class_erases_to_underlying() {
        let Some(out) = run_main(
            r#"
class Meter(val n: Int) extends AnyVal {
  def doubled: Int = n * 2
}
object Main {
  def main(args: Array[String]): Unit = {
    val m = new Meter(21)
    println(m.doubled)
    println(m.n)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "42\n21\n", "stdout: {out:?}");
    }

    #[test]
    fn predef_assert_arrow_stringops() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    assert(true)
    require(1 > 0)
    println("42".length)
    println("42".toInt)
    val t = 1 -> "a"
    println(t._1)
    println(t._2)
    try {
      ???
    } catch {
      case _: Throwable => println("nyi")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "2\n42\n1\na\nnyi\n", "stdout: {out:?}");
    }

    #[test]
    fn unapply_seq_list_and_named() {
        let Some(out) = run_main(
            r#"
object PairSeq {
  def unapplySeq(n: Int): Option[List[Int]] = Some(n :: (n + 1) :: Nil)
}
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: 3 :: Nil
    val s = xs match {
      case List(a, b, c) => a + b + c
      case _ => 0
    }
    println(s)
    val t = 10 match {
      case PairSeq(a, b) => a + b
      case _ => -1
    }
    println(t)
    val h = xs match {
      case List(a, rest @ _*) => a
      case _ => 0
    }
    println(h)
    val p = Point(3, 4) match {
      case Point(y = b, x = a) => a + b
      case _ => 0
    }
    println(p)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "6\n21\n1\n7\n", "stdout: {out:?}");
    }

    #[test]
    fn trait_val_init() {
        let Some(out) = run_main(
            r#"
trait T {
  val msg: String = "from trait"
}
class C extends T
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out.trim(), "from trait", "stdout: {out:?}");
    }

    #[test]
    fn abstract_override_super_chain() {
        let Some(out) = run_main(
            r#"
trait Base {
  def msg: String = "base"
}
trait A extends Base {
  abstract override def msg: String = "A-" + super.msg
}
trait B extends Base {
  abstract override def msg: String = "B-" + super.msg
}
class C extends Base with A with B
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().msg)
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out.trim(), "B-A-base", "stdout: {out:?}");
    }

    #[test]
    fn predef_identity_locally_implicitly_stringadd() {
        let Some(out) = run_main(
            r#"
object Main {
  implicit val n: Int = 41
  def main(args: Array[String]): Unit = {
    println(1 + "x")
    println(implicitly[Int])
    println(identity(42))
    locally {
      println("here")
    }
  }
}
"#,
        ) else {
            return;
        };
        assert_eq!(out, "1x\n41\n42\nhere\n", "stdout: {out:?}");
    }
}

/// The JVM box for a Scala primitive; `x.toString` on an `Int` dispatches on it.
fn is_boxed_primitive(jvm: &str) -> bool {
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

/// `1 + 2.5` reaches `Double.+` with an `int` receiver; the JVM needs the
/// widening instruction before the arithmetic op.
fn widen_numeric(asm: &mut Assembler, from: &Type, to: &Type) {
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
        _ => {}
    }
}

fn append_str(asm: &mut Assembler, s: &str) {
    asm.ldc_string(s);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
}

/// The `StringBuilder.append` overload for a field's erased type.
fn append_desc(ty: &Type) -> &'static str {
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
