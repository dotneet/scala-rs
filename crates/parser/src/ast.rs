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

    /// A tree the typer already typed, spliced back into a macro expansion
    /// unchanged: the receiver or an argument of the macro application, which
    /// the implementation returned as it was given. nsc hands a macro typed
    /// trees and does not type them again, and neither does
    /// `Typer::type_expr`; re-typing one at the call site from its source
    /// shape can resolve differently from how the typer resolved it the first
    /// time (an implicit found in a companion's implicit scope names nothing
    /// in lexical scope).
    pub const PRETYPED_SPLICE: NodeId = NodeId(u32::MAX - 2);

    pub fn is_pretyped_splice(self) -> bool {
        self.0 == u32::MAX - 2
    }

    /// Either kind of tree the typer must not type again.
    pub fn is_pretyped(self) -> bool {
        self.is_pretyped_default() || self.is_pretyped_splice()
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
/// What a type mentions anywhere inside it, as bits: a cheap "no" for the
/// walks that ask whether a type contains a type parameter, a type member,
/// a name still to resolve, and so on.
///
/// Computed when a [`TyList`] or [`TyBox`] is built, so [`Type::flags`]
/// costs one step per direct child rather than a walk of the whole type. A
/// set bit means "may contain": a list or box mutated in place sets every
/// bit, which is always a correct answer, but a lost bit costs whatever
/// cache or shortcut was keyed on it, so the flags are otherwise exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TypeFlags(u16);

impl TypeFlags {
    pub const NONE: TypeFlags = TypeFlags(0);
    pub const TYPE_PARAM: TypeFlags = TypeFlags(1 << 0);
    pub const TYPE_MEMBER: TypeFlags = TypeFlags(1 << 1);
    pub const NAMED: TypeFlags = TypeFlags(1 << 2);
    pub const TUPLE: TypeFlags = TypeFlags(1 << 3);
    /// `p.type` or `C.this.type`.
    pub const SINGLETON: TypeFlags = TypeFlags(1 << 4);
    /// `_`, `_ <: T`, or an existential.
    pub const WILDCARD: TypeFlags = TypeFlags(1 << 5);
    pub const REFINED: TypeFlags = TypeFlags(1 << 6);
    pub const ERROR: TypeFlags = TypeFlags(1 << 7);
    pub const APPLIED: TypeFlags = TypeFlags(1 << 8);
    pub const MODULE_REF: TypeFlags = TypeFlags(1 << 9);
    pub const CONSTANT: TypeFlags = TypeFlags(1 << 10);
    pub const FUNCTION: TypeFlags = TypeFlags(1 << 11);
    pub const ALL: TypeFlags = TypeFlags(u16::MAX);

    #[inline]
    pub fn contains(self, other: TypeFlags) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for TypeFlags {
    type Output = TypeFlags;
    #[inline]
    fn bitor(self, other: TypeFlags) -> TypeFlags {
        TypeFlags(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for TypeFlags {
    #[inline]
    fn bitor_assign(&mut self, other: TypeFlags) {
        self.0 |= other.0;
    }
}

/// The type arguments, parameters or parents a [`Type`] holds: shared, so a
/// clone is a reference count and not a copy of the whole subtree, with the
/// [`TypeFlags`] of its elements computed once.
///
/// It reads as a slice (`Deref<Target = [Type]>`) and is built from a
/// `Vec<Type>` (`.into()`) or by `collect()`. Mutating it in place copies the
/// elements if they are shared, and gives up on the flags (every bit set),
/// which stays correct.
#[derive(Clone)]
pub struct TyList {
    items: std::rc::Rc<Vec<Type>>,
    flags: TypeFlags,
}

impl TyList {
    pub fn new(items: Vec<Type>) -> TyList {
        let flags = items.iter().fold(TypeFlags::NONE, |f, t| f | t.flags());
        TyList {
            items: std::rc::Rc::new(items),
            flags,
        }
    }

    #[inline]
    pub fn flags(&self) -> TypeFlags {
        self.flags
    }

    /// The elements as a vector of their own.
    pub fn into_vec(self) -> Vec<Type> {
        std::rc::Rc::try_unwrap(self.items).unwrap_or_else(|shared| (*shared).clone())
    }

    /// Whether both share one allocation, which makes them equal.
    #[inline]
    pub fn ptr_eq(&self, other: &TyList) -> bool {
        std::rc::Rc::ptr_eq(&self.items, &other.items)
    }
}

impl Default for TyList {
    fn default() -> TyList {
        TyList::new(Vec::new())
    }
}

impl std::ops::Deref for TyList {
    type Target = Vec<Type>;
    #[inline]
    fn deref(&self) -> &Vec<Type> {
        &self.items
    }
}

impl std::ops::DerefMut for TyList {
    fn deref_mut(&mut self) -> &mut Vec<Type> {
        self.flags = TypeFlags::ALL;
        std::rc::Rc::make_mut(&mut self.items)
    }
}

impl PartialEq for TyList {
    fn eq(&self, other: &TyList) -> bool {
        self.ptr_eq(other) || *self.items == *other.items
    }
}

impl PartialEq<Vec<Type>> for TyList {
    fn eq(&self, other: &Vec<Type>) -> bool {
        *self.items == *other
    }
}

impl PartialEq<TyList> for Vec<Type> {
    fn eq(&self, other: &TyList) -> bool {
        *self == *other.items
    }
}

impl fmt::Debug for TyList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.items, f)
    }
}

impl From<Vec<Type>> for TyList {
    fn from(items: Vec<Type>) -> TyList {
        TyList::new(items)
    }
}

impl From<&[Type]> for TyList {
    fn from(items: &[Type]) -> TyList {
        TyList::new(items.to_vec())
    }
}

impl From<TyList> for Vec<Type> {
    fn from(list: TyList) -> Vec<Type> {
        list.into_vec()
    }
}

impl FromIterator<Type> for TyList {
    fn from_iter<I: IntoIterator<Item = Type>>(iter: I) -> TyList {
        TyList::new(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a TyList {
    type Item = &'a Type;
    type IntoIter = std::slice::Iter<'a, Type>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a mut TyList {
    type Item = &'a mut Type;
    type IntoIter = std::slice::IterMut<'a, Type>;
    fn into_iter(self) -> Self::IntoIter {
        use std::ops::DerefMut;
        self.deref_mut().iter_mut()
    }
}

impl IntoIterator for TyList {
    type Item = Type;
    type IntoIter = std::vec::IntoIter<Type>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

/// A single [`Type`] held by another, shared the way [`TyList`] is: the
/// result of a function or method, an array's element, a prefix, a bound.
#[derive(Clone)]
pub struct TyBox {
    ty: std::rc::Rc<Type>,
    flags: TypeFlags,
}

impl TyBox {
    pub fn new(ty: Type) -> TyBox {
        let flags = ty.flags();
        TyBox {
            ty: std::rc::Rc::new(ty),
            flags,
        }
    }

    #[inline]
    pub fn flags(&self) -> TypeFlags {
        self.flags
    }

    /// The type, writable, copied first if it is shared.
    pub fn as_mut(&mut self) -> &mut Type {
        use std::ops::DerefMut;
        self.deref_mut()
    }

    /// The type itself, copied only if it is shared.
    pub fn into_inner(self) -> Type {
        std::rc::Rc::try_unwrap(self.ty).unwrap_or_else(|shared| (*shared).clone())
    }
}

impl std::ops::Deref for TyBox {
    type Target = Type;
    #[inline]
    fn deref(&self) -> &Type {
        &self.ty
    }
}

impl std::ops::DerefMut for TyBox {
    fn deref_mut(&mut self) -> &mut Type {
        self.flags = TypeFlags::ALL;
        std::rc::Rc::make_mut(&mut self.ty)
    }
}

impl AsRef<Type> for TyBox {
    fn as_ref(&self) -> &Type {
        &self.ty
    }
}

impl std::borrow::Borrow<Type> for TyBox {
    fn borrow(&self) -> &Type {
        &self.ty
    }
}

impl PartialEq for TyBox {
    fn eq(&self, other: &TyBox) -> bool {
        std::rc::Rc::ptr_eq(&self.ty, &other.ty) || *self.ty == *other.ty
    }
}

impl fmt::Debug for TyBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.ty, f)
    }
}

impl fmt::Display for TyBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.ty, f)
    }
}

impl From<Type> for TyBox {
    fn from(ty: Type) -> TyBox {
        TyBox::new(ty)
    }
}

impl From<Box<Type>> for TyBox {
    fn from(ty: Box<Type>) -> TyBox {
        TyBox::new(*ty)
    }
}

impl From<TyBox> for Box<Type> {
    fn from(ty: TyBox) -> Box<Type> {
        Box::new(ty.into_inner())
    }
}

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
    /// Object in a Java signature. Reads are references; a Java parameter
    /// (including an Object[] element store) accepts values through boxing.
    JavaObject,
    AnyVal,
    Null,
    Nothing,
    Array(TyBox),
    Tuple(TyList),
    Function {
        params: TyList,
        ret: TyBox,
    },
    /// Named type not yet bound to a symbol (`List[Int]`, user types in tpts).
    Named {
        name: String,
        args: TyList,
    },
    Class {
        sym: SymbolId,
        args: TyList,
    },
    Method {
        paramss: Vec<Vec<Type>>,
        ret: TyBox,
    },
    ByName(TyBox),
    /// Repeated parameter `T*` (erasure: `Seq[T]`).
    Repeated(TyBox),
    Overload(TyList),
    /// Package or module as a prefix (for Select).
    ModuleRef(SymbolId),
    /// A type parameter (`T` in `def id[T](x: T): T`).
    TypeParam(SymbolId),
    /// Application of a higher-kinded type constructor that is not a class
    /// (`F[A]` where `F` is `F[_]`). Class applications stay `Class { args }`.
    Applied {
        ctor: TyBox,
        args: TyList,
    },
    /// Abstract type member (`trait Foo { type A }`). Aliases expand away.
    TypeMember(SymbolId),
    /// Unbounded wildcard existential `_` (as in `List[_]`).
    Wildcard,
    /// Bounded wildcard `_ <: Hi` / `_ >: Lo` (as in `List[_ <: AnyRef]`).
    BoundedWildcard {
        lo: Option<TyBox>,
        hi: Option<TyBox>,
    },
    /// A wildcard quantified outside its body, rather than inside a nested
    /// application. Each parameter is paired with its wildcard bounds.
    Existential {
        params: Vec<(SymbolId, Type)>,
        body: TyBox,
    },
    /// `this.type` of class `cls`.
    ThisType(SymbolId),
    /// Stable path singleton `p.type`. `sym` is the term (`val` / module).
    SingleType {
        prefix: TyBox,
        sym: SymbolId,
    },
    /// SIP-23 literal / constant type (`1`, `true`, `"hi"`). Subtype of the
    /// underlying type (`1 <: Int`). Pickled as nsc `CONSTANTtpe`.
    Constant(Lit),
    /// `T @annot` (type annotation; not a symbol annotation).
    Annotated {
        tpe: TyBox,
        annot: String,
    },
    /// Structural / refinement type (`{ def foo: Int }` or `T { type A = Int }`).
    Refined {
        parents: TyList,
        decls: Vec<RefineDecl>,
    },
}

impl Type {
    /// What this type may mention anywhere inside it; see [`TypeFlags`].
    /// One step per direct child: the lists and boxes below carry theirs.
    pub fn flags(&self) -> TypeFlags {
        let bx = |b: &TyBox| b.flags();
        match self {
            Type::TypeParam(_) => TypeFlags::TYPE_PARAM,
            Type::TypeMember(_) => TypeFlags::TYPE_MEMBER,
            Type::Named { args, .. } => TypeFlags::NAMED | args.flags(),
            Type::Tuple(ts) => TypeFlags::TUPLE | ts.flags(),
            Type::Class { args, .. } => args.flags(),
            Type::Overload(ts) => ts.flags(),
            Type::Function { params, ret } => TypeFlags::FUNCTION | params.flags() | bx(ret),
            Type::Method { paramss, ret } => {
                paramss.iter().flatten().fold(bx(ret), |f, p| f | p.flags())
            }
            Type::Applied { ctor, args } => TypeFlags::APPLIED | bx(ctor) | args.flags(),
            Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => bx(t),
            Type::Annotated { tpe, .. } => bx(tpe),
            Type::SingleType { prefix, .. } => TypeFlags::SINGLETON | bx(prefix),
            Type::ThisType(_) => TypeFlags::SINGLETON,
            Type::ModuleRef(_) => TypeFlags::MODULE_REF,
            Type::Wildcard => TypeFlags::WILDCARD,
            Type::BoundedWildcard { lo, hi } => {
                let mut f = TypeFlags::WILDCARD;
                for b in [lo, hi].into_iter().flatten() {
                    f |= bx(b);
                }
                f
            }
            Type::Existential { params, body } => params
                .iter()
                .fold(TypeFlags::WILDCARD | bx(body), |f, (_, b)| f | b.flags()),
            Type::Refined { parents, decls } => decls
                .iter()
                .fold(TypeFlags::REFINED | parents.flags(), |f, d| f | d.flags()),
            Type::Constant(_) => TypeFlags::CONSTANT,
            Type::Error => TypeFlags::ERROR,
            _ => TypeFlags::NONE,
        }
    }

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
                args: vec![].into(),
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

    /// [`Type::widen_constant`], borrowing everything that is not a constant.
    pub fn widen_constant_cow(&self) -> std::borrow::Cow<'_, Type> {
        match self {
            Type::Constant(lit) => std::borrow::Cow::Owned(Type::lit_underlying(lit)),
            t => std::borrow::Cow::Borrowed(t),
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
            Type::JavaObject => write!(f, "Object"),
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
            Type::Existential { body, .. } => write!(f, "{body} forSome {{ ... }}"),
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
        /// The declaration's **own** type parameters (`def stepper[S <:
        /// Stepper[_]](implicit shape: StepperShape[A, S]): S with
        /// EfficientSplit`, as `StreamExtensions` writes it). Empty for a
        /// monomorphic declaration, which is every other one.
        ///
        /// They are real symbols, so `paramss` and `ret` mention them as
        /// `Type::TypeParam`; `SymbolTable::conforms_to_refinement`
        /// alpha-renames them to a candidate member's own before comparing.
        tparams: Vec<SymbolId>,
        paramss: Vec<Vec<Type>>,
        ret: Type,
    },
    Val {
        name: String,
        ty: Type,
    },
}

impl RefineDecl {
    /// What the declaration's types may mention; see [`TypeFlags`].
    pub fn flags(&self) -> TypeFlags {
        let opt = |t: &Option<Type>| t.as_ref().map_or(TypeFlags::NONE, Type::flags);
        match self {
            RefineDecl::Type { rhs, lo, hi, .. } => opt(rhs) | opt(lo) | opt(hi),
            RefineDecl::Def { paramss, ret, .. } => paramss
                .iter()
                .flatten()
                .fold(ret.flags(), |f, p| f | p.flags()),
            RefineDecl::Val { ty, .. } => ty.flags(),
        }
    }
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
            RefineDecl::Def {
                name,
                tparams,
                paramss,
                ret,
            } => {
                write!(f, "def {name}")?;
                if !tparams.is_empty() {
                    // Symbol ids alone; the table is not to hand here. The
                    // rendering that matters is `SymbolTable::display_refine_decl`.
                    write!(f, "[{}]", vec!["_"; tparams.len()].join(", "))?;
                }
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
    /// A Function generated to delay a by-name argument, never a source literal.
    pub byname_thunk: bool,
    /// Parser-created by-name type head, distinct from a written identifier.
    pub byname_type_marker: bool,
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
            byname_thunk: false,
            byname_type_marker: false,
        }
    }

    pub fn dummy(kind: TreeKind) -> Self {
        Tree::new(NodeId(0), Span::DUMMY, kind)
    }

    /// Value type presented to argument inference before thunk lowering.
    pub fn argument_type(&self) -> Type {
        if self.byname_thunk {
            if let Type::Function { ret, .. } = &self.ty {
                return Type::ByName(ret.clone());
            }
        }
        self.ty.clone()
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.kind, TreeKind::Empty)
    }

    /// Asked of a `ValDef`'s right-hand side: is it the `_` of
    /// `var x: T = _` (nsc `DEFAULTINIT`)? The parser keeps that `_` as the
    /// rhs only for a typed `var` of a plain name, so the definition is
    /// concrete (a field) but has no initializer to run.
    pub fn is_default_init(&self) -> bool {
        matches!(self.kind, TreeKind::Wildcard)
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
    /// Every `if` that follows this enumerator, in source order. Each one
    /// becomes its own `withFilter` (nsc desugars `x <- e if a; if b` to
    /// `e.withFilter(a).withFilter(b)`); keeping only the last one silently
    /// dropped the earlier filters.
    pub guards: Vec<Tree>,
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
    ) || (!c.is_ascii() && scala_rs_lexer::is_unicode_symbol(c))
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
        // nsc `isScalaLetter`: any Unicode letter (`c 𐀀 d`), `$` and `_`.
        c if c.is_alphabetic() || c == '_' || c == '$' => 1,
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

/// nsc's `nme.isVariableName` first-character test, which decides whether an
/// identifier pattern binds or compares: `_`, or a character that is both
/// lower case (`Character.isLowerCase`, which counts `Other_Lowercase` such
/// as `ª` and `ʰ`) and a *letter* (`Character.isLetter`). A lower-case letter
/// *number* such as `ⅰ` is not a letter, so `case ⅰ_ⅲ =>` compares
/// (run/identifierCase). Rust's `is_alphabetic` also admits the `Nl`
/// numbers, hence the `is_numeric` exclusion.
pub fn is_variable_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c == '_' || (c.is_lowercase() && c.is_alphabetic() && !c.is_numeric()))
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
