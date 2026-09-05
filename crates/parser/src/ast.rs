//! Untyped (and later typed) trees, modeled after nsc's `Tree`.
//!
//! Later phases (uncurry, erasure) can rewrite these nodes in place; `ty` and
//! `sym` start empty and are filled by the namer/typer.

use scala_rs_span::Span;
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// An argument the typer put into a call's argument list itself: a
    /// resolved implicit, or a filled-in default. The parser never hands out
    /// this id, so a pass that re-types an application can tell the
    /// arguments the user wrote from the ones a previous pass added, and drop
    /// the latter before resolving the call again.
    pub const FILLED_ARG: NodeId = NodeId(u32::MAX);

    pub fn is_filled_arg(self) -> bool {
        self.0 == u32::MAX
    }

    /// A default argument's right-hand side that the typer already typed, in
    /// the scope the default was *written* in rather than the one the call
    /// happens to sit in. Re-typing such a tree would undo exactly that, so
    /// `Typer::type_expr` leaves it alone; unlike `FILLED_ARG` it stays in the
    /// argument list when an application is resolved a second time, because it
    /// occupies a parameter slot the re-resolution would otherwise mis-count.
    pub const PRETYPED_DEFAULT: NodeId = NodeId(u32::MAX - 1);

    pub fn is_pretyped_default(self) -> bool {
        self.0 == u32::MAX - 1
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

impl SymbolId {
    pub const NONE: SymbolId = SymbolId(0);
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags(pub u32);

impl Flags {
    pub const EMPTY: Flags = Flags(0);
    pub const PRIVATE: Flags = Flags(1 << 0);
    pub const PROTECTED: Flags = Flags(1 << 1);
    pub const ABSTRACT: Flags = Flags(1 << 2);
    pub const FINAL: Flags = Flags(1 << 3);
    pub const SEALED: Flags = Flags(1 << 4);
    pub const IMPLICIT: Flags = Flags(1 << 5);
    pub const LAZY: Flags = Flags(1 << 6);
    pub const OVERRIDE: Flags = Flags(1 << 7);
    pub const CASE: Flags = Flags(1 << 8);
    pub const TRAIT: Flags = Flags(1 << 9);
    pub const MUTABLE: Flags = Flags(1 << 10);
    pub const PARAM: Flags = Flags(1 << 11);
    pub const BYNAME: Flags = Flags(1 << 12);
    pub const DEFAULTPARAM: Flags = Flags(1 << 13);
    pub const SYNTHETIC: Flags = Flags(1 << 14);
    pub const MODULE: Flags = Flags(1 << 15);
    pub const INTERFACE: Flags = Flags(1 << 16);
    pub const ACCESSOR: Flags = Flags(1 << 17);
    pub const CONSTRUCTOR: Flags = Flags(1 << 18);
    pub const PACKAGE: Flags = Flags(1 << 19);
    pub const COVARIANT: Flags = Flags(1 << 20);
    pub const CONTRAVARIANT: Flags = Flags(1 << 21);
    /// nsc `LOCAL`: `private[this]` / `protected[this]`.
    pub const LOCAL: Flags = Flags(1 << 22);
    /// nsc `JAVA` (raw `1L << 20`): Java-defined class/member when we pickle one.
    pub const JAVA: Flags = Flags(1 << 23);
    /// nsc `BRIDGE` (raw `1L << 26`): erasure/mixin bridge method.
    pub const BRIDGE: Flags = Flags(1 << 24);
    /// nsc `VARARGS` (raw `1L << 43`): Scala `T*` / Java `T...` method.
    pub const VARARGS: Flags = Flags(1 << 25);
    /// nsc `PRESUPER`: early field defs (`class C extends { val x = 1 } with T`).
    pub const PRESUPER: Flags = Flags(1 << 26);
    /// JVM `ACC_VOLATILE` (`@volatile` fields).
    pub const VOLATILE: Flags = Flags(1 << 27);
    /// JVM `ACC_TRANSIENT` (`@transient` fields).
    pub const TRANSIENT: Flags = Flags(1 << 28);
    /// JVM `ACC_STATIC` (Java static methods/fields recovered from classfiles).
    pub const STATIC: Flags = Flags(1 << 29);
    /// JVM `ACC_NATIVE` (`@native` methods).
    pub const NATIVE: Flags = Flags(1 << 30);
    /// JVM `ACC_ENUM` (Java enum class / enum constant).
    pub const ENUM: Flags = Flags(1u32 << 31);

    pub fn contains(self, f: Flags) -> bool {
        self.0 & f.0 != 0
    }
    pub fn with(self, f: Flags) -> Flags {
        Flags(self.0 | f.0)
    }
    pub fn set(&mut self, f: Flags, on: bool) {
        if on {
            self.0 |= f.0;
        } else {
            self.0 &= !f.0;
        }
    }
}

#[derive(Clone, Debug)]
pub struct Modifiers {
    pub flags: Flags,
    pub private_within: Option<String>,
    /// `@tailrec` / `@deprecated` / Java `@Override` / `@Deprecated` / others.
    pub annotations: Vec<Tree>,
}

impl Default for Modifiers {
    fn default() -> Self {
        Modifiers {
            flags: Flags::EMPTY,
            private_within: None,
            annotations: Vec::new(),
        }
    }
}

impl Modifiers {
    pub fn new(flags: Flags) -> Self {
        Modifiers {
            flags,
            private_within: None,
            annotations: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Unit,
    Boolean(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Char(char),
    String(String),
    Symbol(String),
    Null,
}

impl fmt::Display for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lit::Unit => write!(f, "()"),
            Lit::Boolean(b) => write!(f, "{b}"),
            Lit::Int(n) => write!(f, "{n}"),
            Lit::Long(n) => write!(f, "{n}L"),
            Lit::Float(n) => write!(f, "{n}f"),
            Lit::Double(n) => write!(f, "{n}"),
            Lit::Char(c) => write!(f, "{c:?}"),
            Lit::String(s) => write!(f, "{s:?}"),
            Lit::Symbol(s) => write!(f, "'{s}"),
            Lit::Null => write!(f, "null"),
        }
    }
}

/// Structural types. `Class` carries a `SymbolId` once named; before that the
/// name is kept in `Named` so the parser can represent type trees as types too.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    NoType,
    Error,
    Unit,
    Boolean,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    Char,
    String,
    Any,
    AnyRef,
    AnyVal,
    Null,
    Nothing,
    Array(Box<Type>),
    Tuple(Vec<Type>),
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },
    /// Named type not yet bound to a symbol (`List[Int]`, user types in tpts).
    Named {
        name: String,
        args: Vec<Type>,
    },
    Class {
        sym: SymbolId,
        args: Vec<Type>,
    },
    Method {
        paramss: Vec<Vec<Type>>,
        ret: Box<Type>,
    },
    ByName(Box<Type>),
    /// Repeated parameter `T*` (erasure: `Seq[T]`).
    Repeated(Box<Type>),
    Overload(Vec<Type>),
    /// Package or module as a prefix (for Select).
    ModuleRef(SymbolId),
    /// A type parameter (`T` in `def id[T](x: T): T`).
    TypeParam(SymbolId),
    /// Application of a higher-kinded type constructor that is not a class
    /// (`F[A]` where `F` is `F[_]`). Class applications stay `Class { args }`.
    Applied {
        ctor: Box<Type>,
        args: Vec<Type>,
    },
    /// Abstract type member (`trait Foo { type A }`). Aliases expand away.
    TypeMember(SymbolId),
    /// Unbounded wildcard existential `_` (as in `List[_]`).
    Wildcard,
    /// Bounded wildcard `_ <: Hi` / `_ >: Lo` (as in `List[_ <: AnyRef]`).
    BoundedWildcard {
        lo: Option<Box<Type>>,
        hi: Option<Box<Type>>,
    },
    /// `this.type` of class `cls`.
    ThisType(SymbolId),
    /// Stable path singleton `p.type`. `sym` is the term (`val` / module).
    SingleType {
        prefix: Box<Type>,
        sym: SymbolId,
    },
    /// SIP-23 literal / constant type (`1`, `true`, `"hi"`). Subtype of the
    /// underlying type (`1 <: Int`). Pickled as nsc `CONSTANTtpe`.
    Constant(Lit),
    /// `T @annot` (type annotation; not a symbol annotation).
    Annotated {
        tpe: Box<Type>,
        annot: String,
    },
    /// Structural / refinement type (`{ def foo: Int }` or `T { type A = Int }`).
    Refined {
        parents: Vec<Type>,
        decls: Vec<RefineDecl>,
    },
}

impl Type {
    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }
    pub fn is_no_type(&self) -> bool {
        matches!(self, Type::NoType)
    }

    pub fn result(&self) -> &Type {
        match self {
            Type::Method { ret, .. } => ret,
            Type::Function { ret, .. } => ret,
            t => t,
        }
    }

    /// Underlying type of a SIP-23 constant (`1` → `Int`).
    pub fn lit_underlying(lit: &Lit) -> Type {
        match lit {
            Lit::Unit => Type::Unit,
            Lit::Boolean(_) => Type::Boolean,
            Lit::Int(_) => Type::Int,
            Lit::Long(_) => Type::Long,
            Lit::Float(_) => Type::Float,
            Lit::Double(_) => Type::Double,
            Lit::Char(_) => Type::Char,
            Lit::String(_) => Type::String,
            Lit::Null => Type::Null,
            Lit::Symbol(_) => Type::Named {
                name: "Symbol".into(),
                args: vec![],
            },
        }
    }

    /// Widen a constant type to its underlying type; other types are cloned.
    pub fn widen_constant(&self) -> Type {
        match self {
            Type::Constant(lit) => Type::lit_underlying(lit),
            t => t.clone(),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::NoType => write!(f, "<notype>"),
            Type::Error => write!(f, "<error>"),
            Type::Unit => write!(f, "Unit"),
            Type::Boolean => write!(f, "Boolean"),
            Type::Byte => write!(f, "Byte"),
            Type::Short => write!(f, "Short"),
            Type::Int => write!(f, "Int"),
            Type::Long => write!(f, "Long"),
            Type::Float => write!(f, "Float"),
            Type::Double => write!(f, "Double"),
            Type::Char => write!(f, "Char"),
            Type::String => write!(f, "String"),
            Type::Any => write!(f, "Any"),
            Type::AnyRef => write!(f, "AnyRef"),
            Type::AnyVal => write!(f, "AnyVal"),
            Type::Null => write!(f, "Null"),
            Type::Nothing => write!(f, "Nothing"),
            Type::Array(t) => write!(f, "Array[{t}]"),
            Type::Tuple(ts) => {
                write!(f, "(")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, ")")
            }
            Type::Function { params, ret } => {
                if params.len() == 1 {
                    write!(f, "{} => {}", params[0], ret)
                } else {
                    write!(f, "(")?;
                    for (i, t) in params.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{t}")?;
                    }
                    write!(f, ") => {ret}")
                }
            }
            Type::Named { name, args } => {
                write!(f, "{name}")?;
                if !args.is_empty() {
                    write!(f, "[")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    write!(f, "]")?;
                }
                Ok(())
            }
            Type::Class { sym, args } => {
                write!(f, "#{}", sym.0)?;
                if !args.is_empty() {
                    write!(f, "[")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    write!(f, "]")?;
                }
                Ok(())
            }
            Type::Method { paramss, ret } => {
                for ps in paramss {
                    write!(f, "(")?;
                    for (i, p) in ps.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{p}")?;
                    }
                    write!(f, ")")?;
                }
                write!(f, "{ret}")
            }
            Type::ByName(t) => write!(f, "=> {t}"),
            Type::Repeated(t) => write!(f, "{t}*"),
            Type::Overload(alts) => {
                write!(f, "<overload ")?;
                for (i, a) in alts.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ">")
            }
            Type::ModuleRef(s) => write!(f, "module#{}", s.0),
            Type::TypeParam(s) => write!(f, "tparam#{}", s.0),
            Type::Applied { ctor, args } => {
                write!(f, "{ctor}")?;
                write!(f, "[")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, "]")
            }
            Type::TypeMember(s) => write!(f, "tmem#{}", s.0),
            Type::Wildcard => write!(f, "_"),
            Type::BoundedWildcard { lo, hi } => {
                write!(f, "_")?;
                if let Some(t) = lo {
                    write!(f, " >: {t}")?;
                }
                if let Some(t) = hi {
                    write!(f, " <: {t}")?;
                }
                Ok(())
            }
            Type::ThisType(s) => write!(f, "this.type(#{})", s.0),
            Type::SingleType { sym, .. } => write!(f, "#{}.type", sym.0),
            Type::Constant(lit) => write!(f, "{lit}"),
            Type::Annotated { tpe, annot } => write!(f, "{tpe} @{annot}"),
            Type::Refined { parents, decls } => {
                if parents.is_empty() {
                    write!(f, "{{ ")?;
                } else {
                    for (i, p) in parents.iter().enumerate() {
                        if i > 0 {
                            write!(f, " with ")?;
                        }
                        write!(f, "{p}")?;
                    }
                    if decls.is_empty() {
                        return Ok(());
                    }
                    write!(f, " {{ ")?;
                }
                for (i, d) in decls.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{d}")?;
                }
                write!(f, " }}")
            }
        }
    }
}

/// A member declared in a refinement (`T { def foo: Int; type A = Int }`).
#[derive(Clone, Debug, PartialEq)]
pub enum RefineDecl {
    Type {
        name: String,
        rhs: Option<Type>,
        /// Kind arity (`type F[_]` → 1). Zero for a proper type member.
        tparams: usize,
        lo: Option<Type>,
        hi: Option<Type>,
    },
    Def {
        name: String,
        paramss: Vec<Vec<Type>>,
        ret: Type,
    },
    Val {
        name: String,
        ty: Type,
    },
}

impl fmt::Display for RefineDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefineDecl::Type {
                name,
                rhs,
                tparams,
                lo,
                hi,
            } => {
                write!(f, "type {name}")?;
                if *tparams > 0 {
                    write!(f, "[")?;
                    for i in 0..*tparams {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "_")?;
                    }
                    write!(f, "]")?;
                }
                if let Some(t) = lo {
                    write!(f, " >: {t}")?;
                }
                if let Some(t) = hi {
                    write!(f, " <: {t}")?;
                }
                if let Some(t) = rhs {
                    write!(f, " = {t}")?;
                }
                Ok(())
            }
            RefineDecl::Def { name, paramss, ret } => {
                write!(f, "def {name}")?;
                for ps in paramss {
                    write!(f, "(")?;
                    for (i, p) in ps.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{p}")?;
                    }
                    write!(f, ")")?;
                }
                write!(f, ": {ret}")
            }
            RefineDecl::Val { name, ty } => write!(f, "val {name}: {ty}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub id: NodeId,
    pub span: Span,
    pub kind: TreeKind,
    pub ty: Type,
    pub sym: SymbolId,
    /// nsc postfix select (`xs toList`, `42 abs`): same-line `expr ident`.
    pub postfix: bool,
    /// An `Ident` the *compiler* made up for a standard-library name that nsc
    /// writes as a fully qualified tree rather than as a name to be resolved.
    /// `gen.mkTuple` builds `scala.TupleN`, so `(a, b)` keeps meaning the tuple
    /// even inside `object Ordering`, which declares `implicit def Tuple2`.
    /// Such a reference is resolved as a member of package `scala` and never
    /// picks up a same-named term from lexical scope.
    ///
    /// It is deliberately *not* set for every synthesized name: nsc's string
    /// interpolation really does emit an unqualified `StringContext`, and
    /// scalac 2.13.16 reports `value s is not a member of String` for a `s"…"`
    /// written where a `def StringContext` is in scope. Only names nsc itself
    /// qualifies belong here.
    pub scala_ref: bool,
    /// SLS 8.1.5: this `Ident` in pattern position is a *stable identifier*
    /// pattern -- it compares the scrutinee with the value the name denotes
    /// instead of binding a fresh variable. The parser sets it for a
    /// backquoted name (``case `f` =>``, stable however the name is spelled);
    /// the type checker sets it once an ordinary name has resolved to a stable
    /// value. Without the mark the backend cannot tell a resolved `val` from a
    /// pattern variable -- both are `SymKind::Term` -- and compiled
    /// `case VAL =>` into a binding that matches everything.
    pub stable_pat: bool,
}

impl Tree {
    pub fn new(id: NodeId, span: Span, kind: TreeKind) -> Self {
        Tree {
            id,
            span,
            kind,
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        }
    }

    pub fn dummy(kind: TreeKind) -> Self {
        Tree::new(NodeId(0), Span::DUMMY, kind)
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.kind, TreeKind::Empty)
    }

    pub fn name(&self) -> Option<&str> {
        match &self.kind {
            TreeKind::Ident { name } => Some(name),
            TreeKind::Select { name, .. } => Some(name),
            TreeKind::ClassDef { name, .. } => Some(name),
            TreeKind::ModuleDef { name, .. } => Some(name),
            TreeKind::ValDef { name, .. } => Some(name),
            TreeKind::DefDef { name, .. } => Some(name),
            TreeKind::TypeDef { name, .. } => Some(name),
            TreeKind::Bind { name, .. } => Some(name),
            TreeKind::SelectFromTypeTree { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Dotted constructor path of an annotation tree (`scala.annotation.tailrec`).
    pub fn annotation_path(&self) -> String {
        match &self.kind {
            TreeKind::Ident { name } => name.clone(),
            TreeKind::Select { qual, name } => {
                let p = qual.annotation_path();
                if p.is_empty() {
                    name.clone()
                } else {
                    format!("{p}.{name}")
                }
            }
            TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => fun.annotation_path(),
            _ => self.name().unwrap_or("").to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum TreeKind {
    Empty,
    PackageDef {
        pid: Box<Tree>,
        stats: Vec<Tree>,
    },
    Import {
        expr: Box<Tree>,
        selectors: Vec<ImportSelector>,
    },
    ClassDef {
        mods: Modifiers,
        name: String,
        tparams: Vec<Tree>,
        ctor_mods: Modifiers,
        vparamss: Vec<Vec<Tree>>,
        impl_: Template,
    },
    ModuleDef {
        mods: Modifiers,
        name: String,
        impl_: Template,
    },
    ValDef {
        mods: Modifiers,
        name: String,
        tpt: Box<Tree>,
        rhs: Box<Tree>,
    },
    DefDef {
        mods: Modifiers,
        name: String,
        tparams: Vec<Tree>,
        vparamss: Vec<Vec<Tree>>,
        tpt: Box<Tree>,
        rhs: Box<Tree>,
    },
    /// Right-hand side of a def macro: `def f: T = macro Impl.method[A]`.
    ///
    /// `impl_ref` is the unresolved reference to the macro implementation
    /// (`Ident`, `Select`, or `TypeApply` of either). It is never an ordinary
    /// expression: nsc's parser also keeps it separate and the typer resolves
    /// it against the *macro implementation* signature rules, not the def's.
    MacroRhs {
        impl_ref: Box<Tree>,
    },
    TypeDef {
        mods: Modifiers,
        name: String,
        tparams: Vec<Tree>,
        rhs: Box<Tree>,
        lo: Option<Box<Tree>>,
        hi: Option<Box<Tree>>,
        /// View bounds `T <% Ordered[T]`. Empty when none. Multiple `<%` are allowed.
        views: Vec<Tree>,
        /// Context bounds `T: ClassTag`. Empty when none. Multiple `: C` are allowed.
        ctx_bounds: Vec<Tree>,
    },
    LabelDef {
        name: String,
        params: Vec<Tree>,
        rhs: Box<Tree>,
    },
    Block {
        stats: Vec<Tree>,
        expr: Box<Tree>,
    },
    If {
        cond: Box<Tree>,
        thenp: Box<Tree>,
        elsep: Box<Tree>,
    },
    Match {
        selector: Box<Tree>,
        cases: Vec<CaseDef>,
    },
    Function {
        vparams: Vec<Tree>,
        body: Box<Tree>,
    },
    Assign {
        lhs: Box<Tree>,
        rhs: Box<Tree>,
    },
    While {
        cond: Box<Tree>,
        body: Box<Tree>,
    },
    DoWhile {
        body: Box<Tree>,
        cond: Box<Tree>,
    },
    Return {
        expr: Box<Tree>,
    },
    Throw {
        expr: Box<Tree>,
    },
    Try {
        block: Box<Tree>,
        catches: Vec<CaseDef>,
        finalizer: Box<Tree>,
    },
    New {
        tpt: Box<Tree>,
    },
    Typed {
        expr: Box<Tree>,
        tpt: Box<Tree>,
    },
    TypeApply {
        fun: Box<Tree>,
        args: Vec<Tree>,
    },
    Apply {
        fun: Box<Tree>,
        args: Vec<Tree>,
    },
    Super {
        qual: Option<String>,
        mix: Option<String>,
    },
    This {
        qual: Option<String>,
    },
    Select {
        qual: Box<Tree>,
        name: String,
    },
    Ident {
        name: String,
    },
    Literal {
        lit: Lit,
    },
    Bind {
        name: String,
        body: Box<Tree>,
    },
    Star {
        elem: Box<Tree>,
    },
    Alternative {
        trees: Vec<Tree>,
    },
    UnApply {
        fun: Box<Tree>,
        args: Vec<Tree>,
    },
    AppliedTypeTree {
        tpt: Box<Tree>,
        args: Vec<Tree>,
    },
    SingletonTypeTree {
        ref_: Box<Tree>,
    },
    /// `T @annot` in type position.
    AnnotatedTypeTree {
        tpt: Box<Tree>,
        annot: Box<Tree>,
    },
    SelectFromTypeTree {
        qual: Box<Tree>,
        name: String,
        /// `true` for `T#A` (type projection). `false` for `T.A` after a type
        /// (path-dependent / nested select from a type).
        hash: bool,
    },
    CompoundTypeTree {
        parents: Vec<Tree>,
        /// Refinement decls (`def` / `val` / `type`) inside `{ ... }`.
        refinements: Vec<Tree>,
    },
    ExistentialTypeTree {
        tpt: Box<Tree>,
        clauses: Vec<Tree>,
    },
    Wildcard,
    InterpolatedString {
        prefix: String,
        parts: Vec<String>,
        args: Vec<Tree>,
    },
    /// Placeholder for syntax we parse enough to reject with a span.
    Unimplemented {
        what: String,
    },
}

#[derive(Clone, Debug)]
pub struct Template {
    pub parents: Vec<Tree>,
    pub self_name: Option<String>,
    pub self_tpt: Option<Box<Tree>>,
    pub body: Vec<Tree>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct CaseDef {
    pub pat: Tree,
    pub guard: Tree,
    pub body: Tree,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImportSelector {
    pub name: String,
    pub rename: Option<String>,
    pub span: Span,
}

impl ImportSelector {
    pub fn wildcard(span: Span) -> Self {
        ImportSelector {
            name: "_".into(),
            rename: None,
            span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Enumerator {
    pub pat: Tree,
    pub rhs: Tree,
    pub is_val: bool, // `p = e` vs `p <- e`
    pub guard: Option<Tree>,
}

/// nsc `Chars.isOperatorPart`.
fn is_operator_part(c: char) -> bool {
    matches!(
        c,
        '~' | '!'
            | '@'
            | '#'
            | '%'
            | '^'
            | '*'
            | '+'
            | '-'
            | '<'
            | '>'
            | '?'
            | ':'
            | '='
            | '&'
            | '|'
            | '/'
            | '\\'
    )
}

/// nsc `nme.isOpAssignmentName`: an operator that ends in `=`, does not start
/// with `=`, is not `!=` / `<=` / `>=`, and begins with an operator character.
/// A *letter*-headed name such as `max=` is not one (nsc gives it the
/// alphabetic precedence instead).
pub fn is_op_assignment_name(op: &str) -> bool {
    let Some(first) = op.chars().next() else {
        return false;
    };
    op.len() > 1
        && op.ends_with('=')
        && first != '='
        && is_operator_part(first)
        && !matches!(op, "!=" | "<=" | ">=")
}

pub fn op_precedence(op: &str) -> i32 {
    // nsc `precedence`: an op-assignment binds *looser* than every other
    // operator. Ranking `+=` with `+` made `n += i + x` parse as
    // `(n += i) + x`, whose left operand is `Unit`; the typer then reached for
    // `any2stringadd` and reported `no matching overload for (String)String`.
    if is_op_assignment_name(op) {
        return 0;
    }
    match op.chars().next().unwrap_or('\0') {
        c if c.is_ascii_alphabetic() || c == '_' => 1,
        '|' => 2,
        '^' => 3,
        '&' => 4,
        '=' | '!' => 5,
        '<' | '>' => 6,
        ':' => 7,
        '+' | '-' => 8,
        '*' | '/' | '%' => 9,
        _ => 10,
    }
}

pub fn is_assignment_op(op: &str) -> bool {
    op.ends_with('=')
        && op.len() > 1
        && op != "<="
        && op != ">="
        && op != "!="
        && op != "=="
        && !op.starts_with('=')
}
