//! Symbols, scopes, and the compilation context.

use scala_rs_parser::{Flags, RefineDecl, SymbolId, Type};

thread_local! {
    /// Type parameters whose upper bound `is_sub_type` is already expanding.
    /// An F-bound (`A <: Rep[A]`) would otherwise recurse forever.
    static EXPANDING_BOUNDS: std::cell::RefCell<Vec<u32>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct BoundGuard(bool);

impl Drop for BoundGuard {
    fn drop(&mut self) {
        if self.0 {
            EXPANDING_BOUNDS.with(|b| {
                b.borrow_mut().pop();
            });
        }
    }
}

/// Returns `None` once the parent walk is implausibly deep, which only
/// happens when the hierarchy has a cycle.
fn enter_depth() -> Option<BoundGuard> {
    EXPANDING_BOUNDS.with(|b| {
        let mut v = b.borrow_mut();
        if v.len() > 200 {
            return None;
        }
        v.push(u32::MAX);
        Some(BoundGuard(true))
    })
}

/// Returns `None` when this parameter's bound is already being expanded.
fn enter_bound(id: SymbolId) -> Option<BoundGuard> {
    EXPANDING_BOUNDS.with(|b| {
        let mut v = b.borrow_mut();
        if v.contains(&id.0) {
            return None;
        }
        v.push(id.0);
        Some(BoundGuard(true))
    })
}

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymKind {
    NoSymbol,
    Package,
    Class,
    Module,
    ModuleClass,
    Method,
    Term,
    TypeParam,
    TypeMember,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intrinsic {
    None,
    Println,
    Print,
    IntBin(&'static str),
    IntUn(&'static str),
    LongBin(&'static str),
    LongUn(&'static str),
    DoubleBin(&'static str),
    DoubleUn(&'static str),
    FloatBin(&'static str),
    FloatUn(&'static str),
    BoolBin(&'static str),
    BoolUn(&'static str),
    StringConcat,
    AnyToString,
    Identity,
    IntToLong,
    IntToDouble,
    IntToByte,
    IntToShort,
    LongToDouble,
    Assert,
    Require,
    NotImplemented,
    StringToInt,
    StringToLong,
    StringToDouble,
    WrapArrowAssoc,
    Locally,
    Any2StringAdd,
    Implicitly,
    /// AnyRef reference equality (`eq`).
    Eq,
    /// AnyRef reference inequality (`ne`).
    Ne,
    /// Universal equality (`Any.==`).
    AnyEq,
    /// Universal inequality (`Any.!=`).
    AnyNe,
    /// `Any.synchronized` (monitor enter/exit around a by-name body).
    Synchronized,
    /// `x.isInstanceOf[T]` / `x.asInstanceOf[T]`: `instanceof` and a
    /// checkcast-or-unbox against the type argument.
    IsInstanceOf,
    AsInstanceOf,
    /// Numeric widening the JVM needs an instruction for.
    IntToFloat,
    LongToFloat,
    FloatToDouble,
    /// `classOf[T]`: a class constant for the erasure of `T`.
    ClassOf,
    /// `Any.getClass`: `Object.getClass`, or the boxed `TYPE` for a primitive.
    GetClass,
    /// `TupleN.apply` — nsc allocates the tuple directly, so no `TupleN$`
    /// module classfile is needed on the private runtime.
    NewTuple(usize),
    /// `Predef.int2Integer` and its seven siblings: box a primitive into its
    /// `java.lang` wrapper. The payload is the JVM descriptor letter of the
    /// primitive (`I`, `J`, `Z`, ...), so the emitter picks the right
    /// `valueOf` even when the argument arrived as a narrower type
    /// (`java.lang.Integer = 'c'` passes a `Char`).
    BoxValue(&'static str),
    /// `Predef.Integer2int` and siblings: the reverse, `Integer.intValue`.
    UnboxValue(&'static str),
}

/// What a `def f = macro Impl.method` binds to.
///
/// nsc stores the equivalent as a pickled `@macroImpl(tree)` annotation on the
/// macro def symbol so that a *separately compiled* macro def can still be
/// expanded. We keep the same three facts, in the form the expander needs:
/// the JVM class that holds the implementation, the method name on it, and
/// whether the def was declared with a `blackbox` or `whitebox` context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroBinding {
    /// JVM internal name of the class holding the implementation, e.g. `M$`.
    /// nsc requires the implementation to be a method of an object, so this is
    /// always a module class.
    pub impl_class: String,
    /// Method name on `impl_class`.
    pub impl_method: String,
    /// `false` for `scala.reflect.macros.whitebox.Context`. Blackbox macros keep
    /// the declared result type; whitebox macros may refine it, which changes
    /// how the call site re-typechecks the expansion.
    pub blackbox: bool,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub owner: SymbolId,
    pub kind: SymKind,
    pub flags: Flags,
    pub ty: Type,
    pub members: Vec<SymbolId>,
    pub jvm_name: String,
    pub intrinsic: Intrinsic,
    /// Constructor / method parameter symbols (flat, first clause).
    pub params: Vec<SymbolId>,
    pub paramss: Vec<Vec<SymbolId>>,
    /// For case classes / classes: constructor parameter field names.
    pub ctor_fields: Vec<SymbolId>,
    pub parents: Vec<Type>,
    pub default_rhs: Option<scala_rs_parser::Tree>,
    /// Class or method type parameters, in order.
    pub tparams: Vec<SymbolId>,
    /// Direct subclasses / objects of a sealed parent (same compilation unit).
    pub children: Vec<SymbolId>,
    /// Self type (`trait T { self: Foo => }`).
    pub self_type: Option<Type>,
    /// The self *alias* a template introduces (`self` in `trait T { self: Foo => }`).
    /// It is scoped to the template that writes it: unlike an ordinary member
    /// it is not inherited, so every place that copies another template's
    /// members into scope has to leave it out — otherwise two components of a
    /// cake that both call their alias `self` collide into an overload.
    pub self_alias: Option<SymbolId>,
    /// Access qualifier `private[C]` / `protected[C]` (`C` is a class or package name).
    /// `private[this]` is `PRIVATE|LOCAL` with this field empty.
    pub private_within: Option<String>,
    /// A `private` member the companion reads. The JVM has no companions, so
    /// nsc widens such a member (`Counter$$step`); we drop `ACC_PRIVATE`.
    pub access_widened: bool,
    /// nsc `scala.LowPriorityImplicits`: `Predef` inherits `intWrapper` &
    /// friends from a superclass, so a conversion declared in `Predef` itself
    /// (`double2Double`) wins the tie for a member both results offer.
    /// `0.5.isNaN` really is `Predef.double2Double(0.5).isNaN()` in scalac, not
    /// `RichDouble.isNaN`. Modelling the superclass would change the emitted
    /// owner of every wrapper call, so the priority is recorded here instead.
    pub low_priority: bool,
    /// Language annotations (`@deprecated(...)`, `@tailrec`, …) copied from modifiers.
    pub annotations: Vec<scala_rs_parser::Tree>,
    /// Lower bound of an abstract/HK type member (`type F[_] >: Lo`).
    pub bound_lo: Option<Type>,
    /// Upper bound of an abstract/HK type member (`type F[_] <: Hi`).
    pub bound_hi: Option<Type>,
    /// For classes defined inside a method: enclosing-method locals the class
    /// reads. Each becomes a private field plus a trailing constructor
    /// parameter (see `anon_capture`).
    pub captures: Vec<SymbolId>,
    /// Set on `def f = macro Impl.method`. Such a symbol has no bytecode: every
    /// call site must be replaced by the implementation's expansion.
    pub macro_impl: Option<MacroBinding>,
}

impl Symbol {
    pub fn is_class_like(&self) -> bool {
        matches!(self.kind, SymKind::Class | SymKind::ModuleClass)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Scope {
    map: HashMap<String, Vec<SymbolId>>,
    /// Owners brought in by a wildcard import (`import p._`) in this scope,
    /// with the names that selector hid (`import p.{X => _, _}`).
    /// A package read from a jar cannot be enumerated up front, so the names
    /// it offers are resolved on demand: see `Checker::expose_unqualified`.
    wildcards: Vec<WildcardImport>,
}

/// `import owner._`, minus the names hidden by `X => _` selectors.
#[derive(Clone, Debug)]
pub struct WildcardImport {
    pub owner: SymbolId,
    pub hidden: Vec<String>,
}

impl WildcardImport {
    pub fn offers(&self, name: &str) -> bool {
        !self.hidden.iter().any(|h| h == name)
    }
}

impl Scope {
    pub fn enter(&mut self, name: &str, id: SymbolId) {
        let slot = self.map.entry(name.to_string()).or_default();
        // One symbol reachable by two routes is still one symbol, not an
        // overload: a template's self alias, for instance, is entered both
        // with the rest of the class's members and by `bind_self_type`.
        if slot.contains(&id) {
            return;
        }
        slot.push(id);
    }

    pub fn enter_wildcard(&mut self, owner: SymbolId, hidden: &[String]) {
        if let Some(w) = self.wildcards.iter_mut().find(|w| w.owner == owner) {
            w.hidden.retain(|h| hidden.iter().any(|n| n == h));
            return;
        }
        self.wildcards.push(WildcardImport {
            owner,
            hidden: hidden.to_vec(),
        });
    }

    pub fn wildcards(&self) -> &[WildcardImport] {
        &self.wildcards
    }

    pub fn lookup(&self, name: &str) -> &[SymbolId] {
        self.map.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }
}

pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub root: SymbolId,
    pub scala_pkg: SymbolId,
    pub predef: SymbolId,
    pub any_sym: SymbolId,
    pub anyref_sym: SymbolId,
    pub anyval_sym: SymbolId,
    pub int_sym: SymbolId,
    pub byte_sym: SymbolId,
    pub short_sym: SymbolId,
    pub char_sym: SymbolId,
    pub long_sym: SymbolId,
    pub float_sym: SymbolId,
    pub double_sym: SymbolId,
    pub boolean_sym: SymbolId,
    pub unit_sym: SymbolId,
    pub string_sym: SymbolId,
    pub array_sym: SymbolId,
    pub option_sym: SymbolId,
    pub some_sym: SymbolId,
    pub none_sym: SymbolId,
    pub list_sym: SymbolId,
    pub nil_sym: SymbolId,
    pub cons_sym: SymbolId,
    pub object_sym: SymbolId,
    /// Enclosing owner while naming/typing.
    pub owner: SymbolId,
    pub this_class: SymbolId,
}

impl SymbolTable {
    pub fn new() -> Self {
        let mut st = SymbolTable {
            symbols: vec![Symbol {
                id: SymbolId(0),
                name: "<none>".into(),
                owner: SymbolId(0),
                kind: SymKind::NoSymbol,
                flags: Flags::EMPTY,
                ty: Type::NoType,
                members: vec![],
                jvm_name: String::new(),
                intrinsic: Intrinsic::None,
                params: vec![],
                paramss: vec![],
                ctor_fields: vec![],
                parents: vec![],
                default_rhs: None,
                tparams: vec![],
                children: vec![],
                self_type: None,
                self_alias: None,
                private_within: None,
                access_widened: false,
                low_priority: false,
                annotations: vec![],
                bound_lo: None,
                bound_hi: None,
                captures: vec![],
                macro_impl: None,
            }],
            scopes: vec![Scope::default()],
            root: SymbolId(0),
            scala_pkg: SymbolId(0),
            predef: SymbolId(0),
            any_sym: SymbolId(0),
            anyref_sym: SymbolId(0),
            anyval_sym: SymbolId(0),
            int_sym: SymbolId(0),
            byte_sym: SymbolId(0),
            short_sym: SymbolId(0),
            char_sym: SymbolId(0),
            long_sym: SymbolId(0),
            float_sym: SymbolId(0),
            double_sym: SymbolId(0),
            boolean_sym: SymbolId(0),
            unit_sym: SymbolId(0),
            string_sym: SymbolId(0),
            array_sym: SymbolId(0),
            option_sym: SymbolId(0),
            some_sym: SymbolId(0),
            none_sym: SymbolId(0),
            list_sym: SymbolId(0),
            nil_sym: SymbolId(0),
            cons_sym: SymbolId(0),
            object_sym: SymbolId(0),
            owner: SymbolId(0),
            this_class: SymbolId(0),
        };
        st.root = st.alloc(
            "<_root_>",
            SymbolId(0),
            SymKind::Package,
            Flags::PACKAGE,
            "scala/runtime",
        );
        st.owner = st.root;
        st
    }

    pub fn alloc(
        &mut self,
        name: impl Into<String>,
        owner: SymbolId,
        kind: SymKind,
        flags: Flags,
        jvm_name: impl Into<String>,
    ) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            id,
            name: name.into(),
            owner,
            kind,
            flags,
            ty: Type::NoType,
            members: vec![],
            jvm_name: jvm_name.into(),
            intrinsic: Intrinsic::None,
            params: vec![],
            paramss: vec![],
            ctor_fields: vec![],
            parents: vec![],
            default_rhs: None,
            tparams: vec![],
            children: vec![],
            self_type: None,
            self_alias: None,
            private_within: None,
            access_widened: false,
            low_priority: false,
            annotations: vec![],
            bound_lo: None,
            bound_hi: None,
            captures: vec![],
            macro_impl: None,
        });
        if !owner.is_none() && owner.0 as usize <= self.symbols.len() {
            if let Some(ow) = self.symbols.get_mut(owner.0 as usize) {
                ow.members.push(id);
            }
        }
        id
    }

    pub fn get(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.symbols[id.0 as usize]
    }

    pub fn enter_in_current(&mut self, name: &str, id: SymbolId) {
        self.scopes.last_mut().unwrap().enter(name, id);
    }

    /// Record `import owner._` in the innermost scope.
    pub fn enter_wildcard_in_current(&mut self, owner: SymbolId, hidden: &[String]) {
        self.scopes
            .last_mut()
            .unwrap()
            .enter_wildcard(owner, hidden);
    }

    /// Owners a wildcard import offers `name`, innermost scope first.
    pub fn wildcard_owners_for(&self, name: &str) -> Vec<SymbolId> {
        let mut out = Vec::new();
        for sc in self.scopes.iter().rev() {
            for w in sc.wildcards() {
                if w.offers(name) && !out.contains(&w.owner) {
                    out.push(w.owner);
                }
            }
        }
        out
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn lookup(&self, name: &str) -> Vec<SymbolId> {
        for sc in self.scopes.iter().rev() {
            let found = sc.lookup(name);
            if !found.is_empty() {
                return found.to_vec();
            }
        }
        Vec::new()
    }

    /// Look a name up in the *type* namespace. Scala keeps terms and types in
    /// separate namespaces, so `val F = asyncF` inside a class parameterized by
    /// `F[_]` must not hide the type parameter from `val u: F[Unit]`. A scope
    /// that binds the name only as a term is therefore skipped, and the search
    /// continues outward.
    ///
    /// A module is only a *fallback*: nsc names the module class of `object X`
    /// `X$`, so an inherited `object JdbcType` never shadows the top-level
    /// `trait JdbcType[T]` in type position. `X` alone still resolves to the
    /// module when nothing in the type namespace carries that name.
    pub fn lookup_type(&self, name: &str) -> Vec<SymbolId> {
        let mut module_fallback: Vec<SymbolId> = Vec::new();
        for sc in self.scopes.iter().rev() {
            let found = sc.lookup(name);
            if found.is_empty() {
                continue;
            }
            if found.iter().any(|&s| self.is_type_namespace(s)) {
                return found.to_vec();
            }
            if module_fallback.is_empty() && found.iter().any(|&s| self.is_module_like(s)) {
                module_fallback = found.to_vec();
            }
        }
        module_fallback
    }

    /// Names that live in the type namespace under their own spelling.
    fn is_type_namespace(&self, s: SymbolId) -> bool {
        matches!(
            self.get(s).kind,
            SymKind::Class | SymKind::TypeParam | SymKind::TypeMember
        )
    }

    fn is_module_like(&self, s: SymbolId) -> bool {
        matches!(self.get(s).kind, SymKind::Module | SymKind::ModuleClass)
    }

    pub fn lookup_member(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut work = vec![owner];
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            let sym = self.get(id);
            for m in &sym.members {
                if self.get(*m).name == name {
                    out.push(*m);
                }
            }
            for m in &sym.parents.clone() {
                if let Some(ps) = self.class_sym_of(m) {
                    work.push(ps);
                }
            }
            if let Some(st) = &sym.self_type {
                if let Some(ps) = self.class_sym_of(st) {
                    work.push(ps);
                }
            }
        }
        out
    }

    /// nsc: a type parameter stands for its upper bound when its members are
    /// looked up (`def f[A <: Comparable[A]](x: A) = x.compareTo(...)`).
    /// Unbounded parameters are left alone so the caller still sees `A`.
    pub fn widen_type_param(&self, ty: &Type) -> Type {
        let mut t = ty.clone();
        for _ in 0..8 {
            let Type::TypeParam(id) = &t else { break };
            match self.get(*id).bound_hi.clone() {
                Some(hi) => t = hi,
                None => return ty.clone(),
            }
        }
        if matches!(t, Type::TypeParam(_)) {
            return ty.clone();
        }
        t
    }

    pub fn class_sym_of(&self, ty: &Type) -> Option<SymbolId> {
        match ty {
            Type::Class { sym, .. } | Type::ModuleRef(sym) => Some(*sym),
            Type::Int => Some(self.int_sym),
            Type::Byte => Some(self.byte_sym),
            Type::Short => Some(self.short_sym),
            Type::Long => Some(self.long_sym),
            Type::Float => Some(self.float_sym),
            Type::Double => Some(self.double_sym),
            Type::Char => Some(self.char_sym),
            Type::Boolean => Some(self.boolean_sym),
            Type::Unit => Some(self.unit_sym),
            Type::String => Some(self.string_sym),
            Type::Any => Some(self.any_sym),
            Type::AnyRef => Some(self.anyref_sym),
            Type::AnyVal => Some(self.anyval_sym),
            Type::Array(_) => Some(self.array_sym),
            // `Null` is a subtype of every reference type; its members are
            // `AnyRef`'s.
            Type::Null => Some(self.anyref_sym),
            // A trait and its companion share a name; in type position the
            // class wins, so `object B extends B` does not become its own
            // parent.
            Type::Named { name, .. } => {
                let found = self.lookup_type(name);
                found
                    .iter()
                    .copied()
                    .find(|s| self.get(*s).kind == SymKind::Class)
                    .or_else(|| found.into_iter().find(|s| self.get(*s).is_class_like()))
            }
            // An unbounded type parameter's members are `Any`'s; a bounded one
            // resolves through its bound, as in nsc.
            Type::TypeParam(id) => match &self.get(*id).bound_hi {
                Some(hi) => self.class_sym_of(hi),
                None => Some(self.any_sym),
            },
            Type::Applied { ctor, .. } => self.class_sym_of(ctor),
            Type::TypeMember(id) => {
                if !self.get(*id).tparams.is_empty() {
                    // A parameterized abstract member is not a class by itself,
                    // but `type C[T] <: TypedType[T]` still offers `TypedType`'s
                    // members, so member lookup goes through the upper bound.
                    // The chase is guarded: `type Self >: this.type <: Self`
                    // and mutually bounded members would otherwise loop.
                    let mut seen = std::collections::HashSet::new();
                    seen.insert(id.0);
                    self.bounded_member_class(*id, &mut seen)
                } else {
                    let seen = self.type_member_as_seen(*id);
                    if matches!(&seen, Type::TypeMember(x) if *x == *id) {
                        self.get(*id)
                            .bound_hi
                            .clone()
                            .as_ref()
                            .and_then(|h| self.class_sym_of(h))
                    } else {
                        self.class_sym_of(&seen)
                    }
                }
            }
            Type::Wildcard | Type::BoundedWildcard { .. } => Some(self.any_sym),
            Type::ThisType(sym) => Some(*sym),
            Type::Constant(lit) => self.class_sym_of(&Type::lit_underlying(lit)),
            Type::SingleType { prefix, sym } => {
                let t = self.get(*sym).ty.clone();
                if t.is_no_type() {
                    self.class_sym_of(prefix)
                } else {
                    self.class_sym_of(&t)
                }
            }
            Type::Annotated { tpe, .. } => self.class_sym_of(tpe),
            Type::Refined { parents, .. } => parents
                .iter()
                .find_map(|p| self.class_sym_of(p))
                .or(Some(self.anyref_sym)),
            Type::Tuple(ts) if !ts.is_empty() => self
                .lookup(&format!("Tuple{}", ts.len()))
                .into_iter()
                .find(|s| self.get(*s).is_class_like()),
            _ => None,
        }
    }

    /// Companion module of a class (same name, `SymKind::Module`, same owner).
    pub fn companion_module(&self, class_id: SymbolId) -> Option<SymbolId> {
        let s = self.get(class_id);
        if s.kind == SymKind::Module {
            return Some(class_id);
        }
        let name = s.name.clone();
        let owner = s.owner;
        self.get(owner)
            .members
            .iter()
            .copied()
            .find(|&m| self.get(m).kind == SymKind::Module && self.get(m).name == name)
    }

    pub fn module_class_of(&self, id: SymbolId) -> SymbolId {
        match self.get(id).ty {
            Type::ModuleRef(c) => c,
            _ => id,
        }
    }

    /// Substitute class type arguments into a member type (`List[Int].head` → `Int`).
    pub fn subst_tparams(&self, owner: SymbolId, args: &[Type], ty: &Type) -> Type {
        let tps = self.get(owner).tparams.clone();
        if tps.is_empty() || args.is_empty() {
            return ty.clone();
        }
        subst_map(ty, &tps, args)
    }

    /// nsc: a *alias* type member is equivalent to (not merely bounded by) its
    /// right-hand side, so `type Scope = Map[K, V]` and `Map[K, V]` are the same
    /// type in both directions. Abstract members (`type T <: Bound`) have no
    /// right-hand side and are left alone; so are higher-kinded aliases, which
    /// only expand once applied (`expand_applied_hk_alias`). The walk is bounded
    /// because a pickled or malformed chain can be cyclic.
    pub fn dealias(&self, ty: &Type) -> Type {
        let mut t = ty.clone();
        let mut seen: Vec<u32> = Vec::new();
        while let Type::TypeMember(id) = &t {
            if seen.contains(&id.0) {
                return ty.clone();
            }
            seen.push(id.0);
            let next = self.type_member_as_seen(*id);
            if next == t {
                break;
            }
            t = next;
        }
        t
    }

    /// Use-site view of a type member: keep higher-kinded aliases as constructors
    /// (`type F[X] = Id[X]` stays `TypeMember` until applied as `F[Int]`).
    pub fn type_member_as_seen(&self, id: SymbolId) -> Type {
        if !self.get(id).tparams.is_empty() {
            Type::TypeMember(id)
        } else {
            match self.get(id).ty.clone() {
                Type::NoType | Type::Error | Type::TypeMember(_) => Type::TypeMember(id),
                other => other,
            }
        }
    }

    /// The class that supplies the members of a parameterized abstract type
    /// member, found by following its upper bound. `type B[T] <: C[T]` and
    /// `type C[T] <: TypedType[T]` together make `TypedType` the answer for `B`.
    /// `seen` stops the walk on recursive or mutually bounded members.
    fn bounded_member_class(
        &self,
        id: SymbolId,
        seen: &mut std::collections::HashSet<u32>,
    ) -> Option<SymbolId> {
        fn head(
            st: &SymbolTable,
            ty: &Type,
            seen: &mut std::collections::HashSet<u32>,
        ) -> Option<SymbolId> {
            match ty {
                Type::Class { sym, .. } => Some(*sym),
                Type::Applied { ctor, .. } => head(st, ctor, seen),
                Type::Annotated { tpe, .. } => head(st, tpe, seen),
                Type::Refined { parents, .. } => parents.iter().find_map(|p| head(st, p, seen)),
                Type::TypeMember(inner) => {
                    if !seen.insert(inner.0) {
                        return None;
                    }
                    st.bounded_member_class(*inner, seen)
                }
                _ => None,
            }
        }
        let info = self.get(id);
        if !matches!(&info.ty, Type::NoType | Type::Error | Type::TypeMember(_)) {
            let rhs = info.ty.clone();
            if let Some(c) = head(self, &rhs, seen) {
                return Some(c);
            }
        }
        let hi = info.bound_hi.clone()?;
        head(self, &hi, seen)
    }

    /// `F[Int]` where `type F[X] = Id[X]` → `Id[Int]`. Abstract `type F[_]` stays applied.
    pub fn expand_applied_hk_alias(&self, ty: Type) -> Type {
        match &ty {
            Type::Applied { ctor, args } => {
                if let Type::TypeMember(id) = ctor.as_ref() {
                    let info = self.get(*id);
                    if !info.tparams.is_empty()
                        && info.tparams.len() == args.len()
                        && !matches!(&info.ty, Type::NoType | Type::Error | Type::TypeMember(_))
                    {
                        return self.subst_tparams(*id, args, &info.ty);
                    }
                }
                ty
            }
            _ => ty,
        }
    }

    /// Remaining kind arity: 0 is a proper type (`*`), 1 is `* -> *`, etc.
    pub fn kind_arity(&self, ty: &Type) -> usize {
        match ty {
            Type::TypeParam(id) | Type::TypeMember(id) => self.get(*id).tparams.len(),
            Type::Class { sym, args } => self.get(*sym).tparams.len().saturating_sub(args.len()),
            Type::Applied { ctor, args } => self.kind_arity(ctor).saturating_sub(args.len()),
            Type::Named { args, .. } => {
                if args.is_empty() {
                    self.class_sym_of(ty)
                        .map(|c| self.get(c).tparams.len())
                        .unwrap_or(0)
                } else {
                    0
                }
            }
            Type::Annotated { tpe, .. } => self.kind_arity(tpe),
            _ => 0,
        }
    }

    /// Kinds of the next type parameters of a type constructor (`F[_]` → `[0]`).
    pub fn tparam_arities(&self, ty: &Type) -> Vec<usize> {
        match ty {
            Type::Class { sym, args } => self
                .get(*sym)
                .tparams
                .iter()
                .skip(args.len())
                .map(|tp| self.get(*tp).tparams.len())
                .collect(),
            Type::TypeParam(id) | Type::TypeMember(id) => self
                .get(*id)
                .tparams
                .iter()
                .map(|tp| self.get(*tp).tparams.len())
                .collect(),
            Type::Applied { ctor, args } => {
                let mut rest = self.tparam_arities(ctor);
                let n = args.len().min(rest.len());
                rest.drain(0..n);
                rest
            }
            Type::Annotated { tpe, .. } => self.tparam_arities(tpe),
            _ => Vec::new(),
        }
    }

    /// Substitute inherited member types using applied parents (`Functor[Id].map`).
    pub fn subst_as_seen_from(&self, recv: &Type, ty: &Type) -> Type {
        fn walk(
            st: &SymbolTable,
            recv: &Type,
            ty: Type,
            seen: &mut std::collections::HashSet<u32>,
        ) -> Type {
            match recv {
                Type::Class { sym, args } => {
                    if !seen.insert(sym.0) {
                        return ty;
                    }
                    let mut t = if args.is_empty() {
                        ty
                    } else {
                        st.subst_tparams(*sym, args, &ty)
                    };
                    for p in st.get(*sym).parents.clone() {
                        // The parent is declared in terms of *this* class's
                        // type parameters, so it has to be instantiated before
                        // it can instantiate anything itself. Without this,
                        // `OptionMapper2[B1, B2, Boolean, P1, P2, R].column`
                        // keeps its `implicit TypedType[BR]` raw instead of
                        // resolving `BR` to `Boolean` through
                        // `OptionMapper[BR, R]`.
                        let p = if args.is_empty() {
                            p
                        } else {
                            st.subst_tparams(*sym, args, &p)
                        };
                        t = walk(st, &p, t, seen);
                    }
                    t
                }
                Type::ModuleRef(sym) => {
                    if !seen.insert(sym.0) {
                        return ty;
                    }
                    let mut t = ty;
                    for p in st.get(*sym).parents.clone() {
                        t = walk(st, &p, t, seen);
                    }
                    t
                }
                Type::Annotated { tpe, .. } => walk(st, tpe, ty, seen),
                // Only heads that `apply_type_ctor` folds may be re-walked: an
                // abstract type-member head (`ColumnType[U]`) folds to the very
                // same `Applied`, so recursing on it would not terminate — and
                // it carries no class parameters to substitute anyway.
                Type::Applied { ctor, args }
                    if matches!(
                        ctor.as_ref(),
                        Type::Class { .. } | Type::Named { .. } | Type::Applied { .. }
                    ) =>
                {
                    let t = apply_type_ctor((**ctor).clone(), args.clone());
                    if matches!(t, Type::Applied { .. }) {
                        // Still applied: the constructor is abstract (a type
                        // member or parameter), so it names no class to walk
                        // into. Recursing here would not terminate.
                        return ty;
                    }
                    walk(st, &t, ty, seen)
                }
                _ => ty,
            }
        }
        let mut seen = std::collections::HashSet::new();
        walk(self, recv, ty.clone(), &mut seen)
    }

    pub fn type_of_class(&self, id: SymbolId) -> Type {
        let s = self.get(id);
        match s.kind {
            SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
            _ => Type::Class {
                sym: id,
                args: vec![],
            },
        }
    }

    /// One of the nine primitive value classes (`scala.Int`, `scala.Unit`, ...).
    ///
    /// Their `jvm_name` records the *box* they erase to (`java/lang/Integer`),
    /// not a class of their own — `scala/Int.class` does not exist. That makes
    /// the field a representation, not an identity: `java.lang.Integer` is a
    /// different Scala type that happens to share the name, so every lookup
    /// that means "the symbol *for* this JVM class" has to skip these.
    pub fn is_primitive_value_class(&self, id: SymbolId) -> bool {
        !id.is_none()
            && [
                self.int_sym,
                self.long_sym,
                self.float_sym,
                self.double_sym,
                self.char_sym,
                self.boolean_sym,
                self.byte_sym,
                self.short_sym,
                self.unit_sym,
            ]
            .contains(&id)
    }

    /// `class C(val x: T) extends AnyVal` — one ctor param, parent AnyVal.
    pub fn is_value_class(&self, id: SymbolId) -> bool {
        if id.is_none() {
            return false;
        }
        let s = self.get(id);
        if s.kind != SymKind::Class
            || s.flags.contains(Flags::TRAIT)
            || s.flags.contains(Flags::INTERFACE)
            || s.ctor_fields.len() != 1
        {
            return false;
        }
        s.parents.iter().any(|p| {
            matches!(p, Type::AnyVal) || self.class_sym_of(p).is_some_and(|c| c == self.anyval_sym)
        })
    }

    pub fn value_class_underlying(&self, id: SymbolId) -> Option<Type> {
        if !self.is_value_class(id) {
            return None;
        }
        let f = self.get(id).ctor_fields[0];
        Some(self.get(f).ty.clone())
    }

    pub fn is_sealed(&self, id: SymbolId) -> bool {
        !id.is_none() && self.get(id).flags.contains(Flags::SEALED)
    }

    /// Concrete leaves of a sealed hierarchy (case classes, objects, non-sealed classes).
    pub fn sealed_leaves(&self, id: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        fn rec(
            st: &SymbolTable,
            id: SymbolId,
            out: &mut Vec<SymbolId>,
            seen: &mut std::collections::HashSet<u32>,
        ) {
            if !seen.insert(id.0) {
                return;
            }
            let children = st.get(id).children.clone();
            if children.is_empty() {
                let s = st.get(id);
                if s.kind == SymKind::Class
                    && (s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::ABSTRACT))
                    && s.flags.contains(Flags::SEALED)
                {
                    return;
                }
                out.push(id);
                return;
            }
            for c in children {
                let cs = st.get(c);
                if cs.flags.contains(Flags::SEALED)
                    && (cs.flags.contains(Flags::TRAIT)
                        || cs.flags.contains(Flags::ABSTRACT)
                        || cs.kind == SymKind::Class)
                    && !cs.children.is_empty()
                {
                    rec(st, c, out, seen);
                } else {
                    out.push(c);
                }
            }
        }
        rec(self, id, &mut out, &mut seen);
        out
    }

    pub fn enclosing_class_named(&self, from: SymbolId, name: &str) -> Option<SymbolId> {
        let mut cur = from;
        while !cur.is_none() {
            let s = self.get(cur);
            let n = s.name.trim_end_matches('$');
            if n == name && s.is_class_like() {
                return Some(cur);
            }
            cur = s.owner;
        }
        None
    }

    /// Base types of `t` (its parents, transitively), most specific first,
    /// with the owning class's type parameters substituted away.
    /// `t` itself is not included.
    pub fn base_type_seq(&self, t: &Type) -> Vec<Type> {
        let mut out: Vec<Type> = Vec::new();
        let mut seen: Vec<Type> = vec![t.clone()];
        let mut queue: std::collections::VecDeque<Type> = std::collections::VecDeque::new();
        queue.push_back(t.clone());
        let mut guard = 0usize;
        while let Some(cur) = queue.pop_front() {
            guard += 1;
            if guard > 256 {
                break;
            }
            let (sym, args) = match &cur {
                Type::Class { sym, args } => (*sym, args.clone()),
                Type::ModuleRef(s) | Type::ThisType(s) => (*s, Vec::new()),
                _ => continue,
            };
            let s = self.get(sym);
            let tps = s.tparams.clone();
            for p in s.parents.clone() {
                let p = subst_tparams_slice(&tps, &args, &p);
                if seen.contains(&p) {
                    continue;
                }
                seen.push(p.clone());
                out.push(p.clone());
                queue.push_back(p);
            }
        }
        out
    }

    /// Least upper bound of two types, used for `[B >: A]` inference and for
    /// varargs element types. Walks the parent chain, so
    /// `lub(Circle, Rect) = Shape` for a sealed `Shape` hierarchy.
    pub fn lub(&self, a: &Type, b: &Type) -> Type {
        let a = a.widen_constant();
        let b = b.widen_constant();
        if a == b {
            return a;
        }
        if a.is_error() || a.is_no_type() || matches!(a, Type::Nothing) {
            return b;
        }
        if b.is_error() || b.is_no_type() || matches!(b, Type::Nothing) {
            return a;
        }
        if self.is_sub_type(&a, &b) {
            return b;
        }
        if self.is_sub_type(&b, &a) {
            return a;
        }
        // Same class constructor, differing arguments: join the arguments.
        if let (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) = (&a, &b) {
            if s1 == s2 && !a1.is_empty() && a1.len() == a2.len() {
                let tparams = self.get(*s1).tparams.clone();
                let contra = a1.iter().enumerate().any(|(i, _)| {
                    tparams
                        .get(i)
                        .map(|&tp| self.get(tp).flags.contains(Flags::CONTRAVARIANT))
                        .unwrap_or(false)
                });
                if !contra {
                    let joined: Vec<Type> = a1
                        .iter()
                        .zip(a2.iter())
                        .map(|(x, y)| self.lub(x, y))
                        .collect();
                    return Type::Class {
                        sym: *s1,
                        args: joined,
                    };
                }
            }
        }
        // Not just `a`'s ancestors: `None` (`<: Option[Nothing]` only) paired
        // with `Some[Boolean]` (`<: Option[Boolean]`) has no match walking
        // only `a`'s chain (`Some[Boolean] <: Option[Nothing]` is false, since
        // `Boolean` is not `<: Nothing`), but walking `b`'s chain finds
        // `Option[Boolean]`, which *does* accept `a` (`Nothing <: Boolean`).
        // A real LUB would also join partial candidates from both sides; this
        // first-match version is simpler but covers the common "singleton
        // case object vs. parameterized case class" pattern, which needs one
        // side's own instantiation to be precise enough already.
        for cand in self.base_type_seq(&a) {
            if matches!(cand, Type::Any | Type::AnyRef | Type::AnyVal) {
                continue;
            }
            if self.is_sub_type(&b, &cand) {
                return cand;
            }
        }
        for cand in self.base_type_seq(&b) {
            if matches!(cand, Type::Any | Type::AnyRef | Type::AnyVal) {
                continue;
            }
            if self.is_sub_type(&a, &cand) {
                return cand;
            }
        }
        if self.is_sub_type(&a, &Type::AnyRef) && self.is_sub_type(&b, &Type::AnyRef) {
            Type::AnyRef
        } else {
            Type::Any
        }
    }

    /// SLS 6.26.1: an `Int` literal in range converts to `Byte`, `Short` or
    /// `Char`. This is *narrowing*, not conformance -- overload resolution
    /// only falls back on it, so `sb.append(42)` still picks `append(Int)`.
    pub fn narrows_to(&self, from: &Type, to: &Type) -> bool {
        let Type::Constant(scala_rs_parser::Lit::Int(v)) = from else {
            return false;
        };
        match to {
            Type::Byte => (-128..=127).contains(v),
            Type::Short => (-32768..=32767).contains(v),
            Type::Char => (0..=65535).contains(v),
            _ => false,
        }
    }

    pub fn is_sub_type(&self, a: &Type, b: &Type) -> bool {
        if a == b {
            return true;
        }
        // An alias type member stands for its right-hand side on either side of
        // `<:`. This has to happen before the arms below, because the `Class`
        // and `Applied` arms match without ever looking at `b`.
        if matches!(a, Type::TypeMember(_)) {
            let d = self.dealias(a);
            if d != *a {
                return self.is_sub_type(&d, b);
            }
        }
        if matches!(b, Type::TypeMember(_)) {
            let d = self.dealias(b);
            if d != *b {
                return self.is_sub_type(a, &d);
            }
        }
        match (a, b) {
            (Type::Error, _) | (_, Type::Error) => true,
            (Type::Nothing, _) => true,
            (_, Type::Any) => true,
            (Type::Constant(a), Type::Constant(b)) => a == b,
            (Type::Constant(a), b) => self.is_sub_type(&Type::lit_underlying(a), b),
            (
                Type::Null,
                Type::AnyRef
                | Type::String
                | Type::Array(_)
                | Type::Class { .. }
                | Type::ModuleRef(_)
                | Type::Refined { .. }
                | Type::ThisType(_)
                | Type::SingleType { .. }
                | Type::Annotated { .. },
            ) => true,
            (
                Type::Int
                | Type::Long
                | Type::Double
                | Type::Boolean
                | Type::Byte
                | Type::Short
                | Type::Unit
                | Type::Char
                | Type::Float,
                Type::AnyVal,
            ) => true,
            (
                Type::String
                | Type::Array(_)
                | Type::Class { .. }
                | Type::ModuleRef(_)
                | Type::Function { .. }
                | Type::Refined { .. }
                | Type::ThisType(_)
                | Type::SingleType { .. }
                | Type::Annotated { .. }
                | Type::Applied { .. },
                Type::AnyRef,
            ) => true,
            (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) if s1 == s2 => {
                if a1.is_empty() || a2.is_empty() {
                    true
                } else if a1.len() == a2.len() {
                    let tparams = self.get(*s1).tparams.clone();
                    a1.iter().zip(a2.iter()).enumerate().all(|(i, (x, y))| {
                        let flags = tparams
                            .get(i)
                            .map(|&tp| self.get(tp).flags)
                            .unwrap_or(Flags::EMPTY);
                        if flags.contains(Flags::CONTRAVARIANT) {
                            self.is_sub_type(y, x)
                        } else if flags.contains(Flags::COVARIANT) {
                            self.is_sub_type(x, y)
                        } else if is_wildcard_arg(x) || is_wildcard_arg(y) {
                            // An invariant parameter still *contains* a
                            // wildcard: `List[Byte]` is a
                            // `Collection[_ <: Number]`.
                            self.is_sub_type(x, y)
                        } else {
                            // Invariant: `A[Int]` is not an `A[Any]`.
                            self.is_sub_type(x, y) && self.is_sub_type(y, x)
                        }
                    })
                } else {
                    false
                }
            }
            (Type::Applied { ctor: c1, args: a1 }, Type::Applied { ctor: c2, args: a2 })
                if a1.len() == a2.len() =>
            {
                self.is_sub_type(c1, c2)
                    && a1
                        .iter()
                        .zip(a2.iter())
                        .all(|(x, y)| self.is_sub_type(x, y))
            }
            (Type::Applied { ctor, args }, other) => {
                let folded = apply_type_ctor((**ctor).clone(), args.clone());
                if let Type::Applied { ctor, .. } = &folded {
                    if let Type::TypeMember(id) = ctor.as_ref() {
                        if let Some(hi) = self.get(*id).bound_hi.clone() {
                            return self.is_sub_type(&hi, other);
                        }
                    }
                    false
                } else {
                    self.is_sub_type(&folded, other)
                }
            }
            (other, Type::Applied { ctor, args }) => {
                let folded = apply_type_ctor((**ctor).clone(), args.clone());
                if matches!(folded, Type::Applied { .. }) {
                    false
                } else {
                    self.is_sub_type(other, &folded)
                }
            }
            // Before the Class-parent walk: that arm matches every Class and
            // would otherwise hide `Tuple2[A, B] <: (A, B)` / the reverse.
            (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
                a.iter().zip(b.iter()).all(|(x, y)| self.is_sub_type(x, y))
            }
            (Type::Tuple(ts), Type::Class { sym, args })
                if self.is_tuple_arity(*sym, ts.len())
                    && (args.is_empty() || args.len() == ts.len()) =>
            {
                args.is_empty()
                    || ts
                        .iter()
                        .zip(args.iter())
                        .all(|(x, y)| self.is_sub_type(x, y))
            }
            (Type::Class { sym, args }, Type::Tuple(ts))
                if self.is_tuple_arity(*sym, ts.len())
                    && (args.is_empty() || args.len() == ts.len()) =>
            {
                args.is_empty()
                    || args
                        .iter()
                        .zip(ts.iter())
                        .all(|(x, y)| self.is_sub_type(x, y))
            }
            (a, Type::Refined { parents, decls }) => {
                parents.iter().all(|p| self.is_sub_type(a, p))
                    && self.conforms_to_refinement(a, decls)
            }
            // Wildcards before the Class-parent walk: that arm matches every Class
            // and would otherwise treat `Byte <: List[_ <: Byte]` as "walk Byte's
            // parents" instead of the bound.
            (_, Type::Wildcard) => true,
            (Type::Wildcard, Type::AnyRef | Type::AnyVal) => true,
            (a, Type::BoundedWildcard { hi, .. }) => match hi {
                Some(h) => self.is_sub_type(a, h),
                None => true,
            },
            (Type::BoundedWildcard { hi, .. }, b) => match hi {
                Some(h) => self.is_sub_type(h, b),
                None => matches!(b, Type::Any | Type::AnyRef | Type::Wildcard),
            },
            (Type::Class { sym: s1, args: a1 }, b) => {
                // A malformed hierarchy (`object B extends B`) would otherwise
                // walk its own parents forever. Depth, not identity: a legitimate
                // walk revisits a class at a different type argument.
                let Some(_g) = enter_depth() else {
                    return false;
                };
                let child = self.get(*s1);
                let tps = child.tparams.clone();
                let parents = child.parents.clone();
                parents.iter().any(|p| {
                    let p = subst_tparams_slice(&tps, a1, p);
                    self.is_sub_type(&p, b)
                })
            }
            // `Array` is invariant: scalac rejects an `Array[Int]` where an
            // `Array[Any]` is asked for. A wildcard argument still *contains*
            // the other (`Array[Byte]` is an `Array[_ <: AnyVal]`), same rule
            // as for an invariant class parameter above.
            (Type::Array(x), Type::Array(y)) => {
                if is_wildcard_arg(x) || is_wildcard_arg(y) {
                    self.is_sub_type(x, y)
                } else {
                    self.is_sub_type(x, y) && self.is_sub_type(y, x)
                }
            }
            (Type::ModuleRef(s), Type::Class { sym, .. }) if s == sym => true,
            (Type::ModuleRef(s), b) => {
                let Some(_g) = enter_depth() else {
                    return false;
                };
                self.get(*s)
                    .parents
                    .clone()
                    .iter()
                    .any(|p| self.is_sub_type(p, b))
            }
            (Type::TypeParam(a), Type::TypeParam(b)) if a == b => true,
            (Type::TypeMember(a), Type::TypeMember(b)) if a == b => true,
            (Type::TypeMember(id), b) => {
                if let Some(hi) = self.get(*id).bound_hi.clone() {
                    if let Some(_g) = enter_bound(*id) {
                        if self.is_sub_type(&hi, b) {
                            return true;
                        }
                    }
                }
                matches!(b, Type::AnyRef | Type::AnyVal | Type::Any)
            }
            // `def f[A <: Named](x: A)` may use `x` where a `Named` is wanted.
            (Type::TypeParam(id), b) => {
                if let Some(hi) = self.get(*id).bound_hi.clone() {
                    // `A <: Rep[A]` must not expand its own bound again.
                    if let Some(_g) = enter_bound(*id) {
                        if self.is_sub_type(&hi, b) {
                            return true;
                        }
                    }
                }
                matches!(b, Type::AnyRef | Type::AnyVal)
            }
            (Type::ThisType(s), b) => {
                if matches!(b, Type::ThisType(t) if t == s) {
                    true
                } else {
                    self.is_sub_type(&self.type_of_class(*s), b)
                }
            }
            (Type::SingleType { sym, prefix }, b) => {
                if matches!(b, Type::SingleType { sym: s2, .. } if s2 == sym) {
                    true
                } else {
                    let t = self.get(*sym).ty.clone();
                    if t.is_no_type() {
                        self.is_sub_type(prefix, b)
                    } else {
                        self.is_sub_type(&t, b)
                    }
                }
            }
            (a, Type::Annotated { tpe, .. }) => self.is_sub_type(a, tpe),
            (Type::Annotated { tpe, .. }, b) => self.is_sub_type(tpe, b),
            (
                Type::Function {
                    params: p1,
                    ret: r1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                },
            ) if p1.len() == p2.len() => {
                p2.iter()
                    .zip(p1.iter())
                    .all(|(exp, act)| self.is_sub_type(exp, act))
                    && self.is_sub_type(r1, r2)
            }
            (Type::ByName(a), Type::ByName(b)) => self.is_sub_type(a, b),
            (Type::Repeated(a), Type::Repeated(b)) => self.is_sub_type(a, b),
            (Type::Refined { parents, .. }, b) => parents.iter().any(|p| self.is_sub_type(p, b)),
            _ => false,
        }
    }

    fn is_tuple_arity(&self, sym: SymbolId, n: usize) -> bool {
        let s = self.get(sym);
        let name = s.name.trim_end_matches('$');
        if name == format!("Tuple{n}") {
            return true;
        }
        let jvm = if s.jvm_name.is_empty() {
            String::new()
        } else {
            s.jvm_name.clone()
        };
        jvm == format!("scala/Tuple{n}")
    }

    pub fn display_type(&self, ty: &Type) -> String {
        match ty {
            Type::Class { sym, args } => {
                let mut s = self.get(*sym).name.clone();
                if !args.is_empty() {
                    s.push('[');
                    s.push_str(
                        &args
                            .iter()
                            .map(|a| self.display_type(a))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    s.push(']');
                }
                s
            }
            Type::ModuleRef(id) => self.get(*id).name.clone(),
            Type::TypeParam(id) => self.get(*id).name.clone(),
            Type::Applied { ctor, args } => {
                let mut s = self.display_type(ctor);
                s.push('[');
                s.push_str(
                    &args
                        .iter()
                        .map(|a| self.display_type(a))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                s.push(']');
                s
            }
            Type::TypeMember(id) => {
                let s = self.get(*id);
                format!("{}.{}", self.get(s.owner).name, s.name)
            }
            Type::ThisType(id) => format!("{}.this.type", self.get(*id).name),
            Type::Constant(lit) => format!("{lit}"),
            Type::SingleType { sym, .. } => format!("{}.type", self.get(*sym).name),
            Type::Annotated { tpe, annot } => {
                format!("{} @{}", self.display_type(tpe), annot)
            }
            Type::BoundedWildcard { lo, hi } => {
                let mut s = String::from("_");
                if let Some(t) = lo {
                    s.push_str(" >: ");
                    s.push_str(&self.display_type(t));
                }
                if let Some(t) = hi {
                    s.push_str(" <: ");
                    s.push_str(&self.display_type(t));
                }
                s
            }
            Type::Refined { parents, decls } => {
                let mut s = String::new();
                if parents.is_empty() {
                    s.push_str("{ ");
                } else {
                    for (i, p) in parents.iter().enumerate() {
                        if i > 0 {
                            s.push_str(" with ");
                        }
                        s.push_str(&self.display_type(p));
                    }
                    if decls.is_empty() {
                        return s;
                    }
                    s.push_str(" { ");
                }
                for (i, d) in decls.iter().enumerate() {
                    if i > 0 {
                        s.push_str("; ");
                    }
                    s.push_str(&d.to_string());
                }
                s.push_str(" }");
                s
            }
            Type::Array(t) => format!("Array[{}]", self.display_type(t)),
            Type::Method { paramss, ret } => {
                let mut s = String::new();
                for ps in paramss {
                    s.push('(');
                    s.push_str(
                        &ps.iter()
                            .map(|p| self.display_type(p))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    s.push(')');
                }
                s.push_str(&self.display_type(ret));
                s
            }
            Type::Function { params, ret } => {
                let p = params
                    .iter()
                    .map(|p| self.display_type(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({}) => {}", p, self.display_type(ret))
            }
            Type::Overload(alts) => format!(
                "<overload {}>",
                alts.iter()
                    .map(|a| self.display_type(a))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
            Type::Repeated(t) => format!("{}*", self.display_type(t)),
            Type::ByName(t) => format!("=> {}", self.display_type(t)),
            other => other.to_string(),
        }
    }

    pub fn jvm_internal(&self, id: SymbolId) -> String {
        let s = self.get(id);
        if !s.jvm_name.is_empty() {
            return s.jvm_name.clone();
        }
        // walk owners
        let mut parts = vec![s.name.clone()];
        let mut o = s.owner;
        while !o.is_none() && self.get(o).kind == SymKind::Package && self.get(o).name != "<_root_>"
        {
            parts.push(self.get(o).name.clone());
            o = self.get(o).owner;
        }
        parts.reverse();
        parts.join("/")
    }

    /// `from` and the class-like symbols lexically enclosing it, innermost first.
    fn enclosing_classes(&self, from: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut cur = from;
        while !cur.is_none() {
            if self.get(cur).is_class_like() || out.is_empty() {
                out.push(cur);
            }
            if out.len() > 16 {
                break;
            }
            let owner = self.get(cur).owner;
            if owner == cur {
                break;
            }
            cur = owner;
        }
        out
    }

    /// Replace abstract type members with aliases defined on `from` (and parents).
    pub fn expand_type_members(&self, from: SymbolId, ty: &Type) -> Type {
        match ty {
            Type::TypeMember(id) => {
                // Refinement placeholders are allocated with no owner. Do not
                // replace them with the parent's abstract member of the same name
                // (`{ type A <: Int }` must keep `A`'s bound).
                if self.get(*id).owner.is_none() {
                    return ty.clone();
                }
                let name = self.get(*id).name.clone();
                // `from` first, then its lexically enclosing classes: an inner
                // class (`Main.factory: Main.Factory`) sees `Main`'s implementation
                // of an abstract member declared beside `Factory`, which is what
                // nsc reaches through the outer-instance prefix.
                for owner in self.enclosing_classes(from) {
                    for m in self.lookup_member(owner, &name) {
                        if self.get(m).kind == SymKind::TypeMember {
                            if !self.get(m).tparams.is_empty() {
                                return Type::TypeMember(m);
                            }
                            let t = self.get(m).ty.clone();
                            if matches!(t, Type::TypeMember(_) | Type::NoType | Type::Error) {
                                return Type::TypeMember(m);
                            }
                            return self.expand_type_members(from, &t);
                        }
                    }
                }
                ty.clone()
            }
            Type::Class { sym, args } => Type::Class {
                sym: *sym,
                args: args
                    .iter()
                    .map(|a| self.expand_type_members(from, a))
                    .collect(),
            },
            Type::Applied { ctor, args } => {
                let applied = apply_type_ctor(
                    self.expand_type_members(from, ctor),
                    args.iter()
                        .map(|a| self.expand_type_members(from, a))
                        .collect(),
                );
                self.expand_applied_hk_alias(applied)
            }
            Type::Array(t) => Type::Array(Box::new(self.expand_type_members(from, t))),
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.expand_type_members(from, p))
                    .collect(),
                ret: Box::new(self.expand_type_members(from, ret)),
            },
            Type::Method { paramss, ret } => Type::Method {
                paramss: paramss
                    .iter()
                    .map(|ps| {
                        ps.iter()
                            .map(|p| self.expand_type_members(from, p))
                            .collect()
                    })
                    .collect(),
                ret: Box::new(self.expand_type_members(from, ret)),
            },
            Type::ByName(t) => Type::ByName(Box::new(self.expand_type_members(from, t))),
            Type::Repeated(t) => Type::Repeated(Box::new(self.expand_type_members(from, t))),
            Type::Tuple(ts) => Type::Tuple(
                ts.iter()
                    .map(|t| self.expand_type_members(from, t))
                    .collect(),
            ),
            Type::Refined { parents, decls } => Type::Refined {
                parents: parents
                    .iter()
                    .map(|p| self.expand_type_members(from, p))
                    .collect(),
                decls: decls
                    .iter()
                    .map(|d| expand_refine_decl(self, from, d))
                    .collect(),
            },
            Type::Annotated { tpe, annot } => Type::Annotated {
                tpe: Box::new(self.expand_type_members(from, tpe)),
                annot: annot.clone(),
            },
            Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
                lo: lo
                    .as_ref()
                    .map(|t| Box::new(self.expand_type_members(from, t))),
                hi: hi
                    .as_ref()
                    .map(|t| Box::new(self.expand_type_members(from, t))),
            },
            Type::SingleType { prefix, sym } => Type::SingleType {
                prefix: Box::new(self.expand_type_members(from, prefix)),
                sym: *sym,
            },
            other => other.clone(),
        }
    }

    /// Expand type members using aliases on a (possibly refined) prefix type.
    pub fn expand_in_type(&self, from: &Type, ty: &Type) -> Type {
        match from {
            Type::Refined { parents, decls } => {
                let mut t = subst_refine_aliases(self, decls, ty);
                for p in parents {
                    t = self.expand_in_type(p, &t);
                }
                t
            }
            Type::Class { sym, .. } | Type::ModuleRef(sym) => self.expand_type_members(*sym, ty),
            Type::ThisType(sym) => self.expand_type_members(*sym, ty),
            Type::Annotated { tpe, .. } => self.expand_in_type(tpe, ty),
            Type::SingleType { prefix, sym } => {
                let t = self.get(*sym).ty.clone();
                if t.is_no_type() {
                    self.expand_in_type(prefix, ty)
                } else {
                    self.expand_in_type(&t, ty)
                }
            }
            _ => {
                if let Some(c) = self.class_sym_of(from) {
                    self.expand_type_members(c, ty)
                } else {
                    ty.clone()
                }
            }
        }
    }

    pub fn refine_member_type(decls: &[RefineDecl], name: &str) -> Option<Type> {
        for d in decls {
            match d {
                RefineDecl::Def {
                    name: n,
                    paramss,
                    ret,
                } if n == name => {
                    return Some(Type::Method {
                        paramss: paramss.clone(),
                        ret: Box::new(ret.clone()),
                    });
                }
                RefineDecl::Val { name: n, ty } if n == name => return Some(ty.clone()),
                RefineDecl::Type { name: n, rhs, .. } if n == name => {
                    return Some(rhs.clone().unwrap_or(Type::Named {
                        name: n.clone(),
                        args: vec![],
                    }));
                }
                _ => {}
            }
        }
        None
    }

    pub fn refined_has_term_members(decls: &[RefineDecl]) -> bool {
        decls
            .iter()
            .any(|d| matches!(d, RefineDecl::Def { .. } | RefineDecl::Val { .. }))
    }

    fn conforms_to_refinement(&self, a: &Type, decls: &[RefineDecl]) -> bool {
        for d in decls {
            match d {
                RefineDecl::Type {
                    name,
                    rhs,
                    tparams,
                    hi,
                    ..
                } => {
                    let Some(have) = self.lookup_type_member_on(a, name) else {
                        return false;
                    };
                    if *tparams > 0 && self.kind_arity(&have) != *tparams {
                        return false;
                    }
                    if let Some(want) = rhs {
                        // Abstract `{ type A <: T }` / `{ type F[_] }` store a
                        // TypeMember placeholder; only aliases constrain equality.
                        let abstract_placeholder = match want {
                            Type::TypeMember(id) => matches!(
                                &self.get(*id).ty,
                                Type::TypeMember(_) | Type::NoType | Type::Error
                            ),
                            _ => false,
                        };
                        if !abstract_placeholder {
                            if *tparams > 0 {
                                let args: Vec<Type> = (0..*tparams).map(|_| Type::Int).collect();
                                let have_app = self.expand_applied_hk_alias(apply_type_ctor(
                                    have.clone(),
                                    args.clone(),
                                ));
                                let want_app = self
                                    .expand_applied_hk_alias(apply_type_ctor(want.clone(), args));
                                if !self.types_same_enough(&have_app, &want_app) {
                                    return false;
                                }
                            } else if !self.types_same_enough(&have, want) {
                                return false;
                            }
                        }
                    }
                    if let Some(h) = hi {
                        if *tparams > 0 {
                            let args: Vec<Type> = (0..*tparams).map(|_| Type::Int).collect();
                            let have_app =
                                self.expand_applied_hk_alias(apply_type_ctor(have.clone(), args));
                            if !self.is_sub_type(&have_app, h) {
                                return false;
                            }
                        } else if !self.is_sub_type(&have, h) {
                            return false;
                        }
                    }
                }
                RefineDecl::Def { name, ret, .. } => {
                    let Some(have) = self.lookup_term_member_on(a, name) else {
                        return false;
                    };
                    if !self.is_sub_type(have.result(), ret) {
                        return false;
                    }
                }
                RefineDecl::Val { name, ty } => {
                    let Some(have) = self.lookup_term_member_on(a, name) else {
                        return false;
                    };
                    if !self.is_sub_type(have.result(), ty) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn types_same_enough(&self, a: &Type, b: &Type) -> bool {
        a == b || (self.is_sub_type(a, b) && self.is_sub_type(b, a))
    }

    pub(crate) fn lookup_type_member_on(&self, ty: &Type, name: &str) -> Option<Type> {
        if let Type::Refined { parents, decls } = ty {
            if let Some(t) = Self::refine_member_type(decls, name) {
                if decls
                    .iter()
                    .any(|d| matches!(d, RefineDecl::Type { name: n, .. } if n == name))
                {
                    return Some(t);
                }
            }
            for p in parents {
                if let Some(t) = self.lookup_type_member_on(p, name) {
                    return Some(t);
                }
            }
        }
        let cls = self.class_sym_of(ty)?;
        let found = self.lookup_member(cls, name);
        for m in found {
            if self.get(m).kind == SymKind::TypeMember {
                if !self.get(m).tparams.is_empty() {
                    return Some(self.expand_in_type(ty, &Type::TypeMember(m)));
                }
                let rhs = self.get(m).ty.clone();
                return Some(match rhs {
                    Type::NoType | Type::Error | Type::TypeMember(_) => {
                        self.expand_in_type(ty, &Type::TypeMember(m))
                    }
                    other => self.expand_in_type(ty, &other),
                });
            }
        }
        None
    }

    fn lookup_term_member_on(&self, ty: &Type, name: &str) -> Option<Type> {
        if let Type::Refined { parents, decls } = ty {
            if decls.iter().any(|d| {
                matches!(
                    d,
                    RefineDecl::Def { name: n, .. } | RefineDecl::Val { name: n, .. } if n == name
                )
            }) {
                return Self::refine_member_type(decls, name);
            }
            for p in parents {
                if let Some(t) = self.lookup_term_member_on(p, name) {
                    return Some(t);
                }
            }
        }
        let cls = self.class_sym_of(ty)?;
        self.lookup_member(cls, name).into_iter().find_map(|m| {
            let s = self.get(m);
            match s.kind {
                SymKind::Method | SymKind::Term => Some(self.expand_in_type(ty, &s.ty)),
                _ => None,
            }
        })
    }

    /// SIP-21: exactly one abstract method (not an Object method / FunctionN).
    pub fn sam_sig(&self, ty: &Type) -> Option<SamSig> {
        let cls = self.class_sym_of(ty)?;
        let jvm = self.get(cls).jvm_name.clone();
        if jvm.starts_with("scala/Function") || jvm.ends_with("PartialFunction") {
            return None;
        }
        let abstracts = self.abstract_sam_methods(cls);
        if abstracts.len() != 1 {
            return None;
        }
        let method = abstracts[0];
        let targs: Vec<Type> = match ty {
            Type::Class { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let tps = self.get(cls).tparams.clone();
        let subst = |t: &Type| subst_tparams_slice(&tps, &targs, t);
        let (raw_params, raw_ret) = match &self.get(method).ty {
            Type::Method { paramss, ret } => (
                paramss.iter().flatten().cloned().collect::<Vec<_>>(),
                (**ret).clone(),
            ),
            _ => return None,
        };
        Some(SamSig {
            class: cls,
            method,
            name: self.get(method).name.clone(),
            param_tys: raw_params.iter().map(subst).collect(),
            ret_ty: subst(&raw_ret),
            raw_param_tys: raw_params,
            raw_ret_ty: raw_ret,
        })
    }

    fn abstract_sam_methods(&self, cls: SymbolId) -> Vec<SymbolId> {
        let mut by_name: HashMap<String, SymbolId> = HashMap::new();
        let mut work = vec![cls];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            for m in &self.get(id).members {
                let s = self.get(*m);
                if s.kind != SymKind::Method || sam_excluded_name(&s.name) {
                    continue;
                }
                by_name.entry(s.name.clone()).or_insert(*m);
            }
            for p in self.get(id).parents.clone() {
                if let Some(c) = self.class_sym_of(&p) {
                    work.push(c);
                }
            }
        }
        by_name
            .into_values()
            .filter(|m| self.get(*m).flags.contains(Flags::ABSTRACT))
            .collect()
    }
}

/// SAM conversion target (class + single abstract method).
#[derive(Clone, Debug)]
pub struct SamSig {
    pub class: SymbolId,
    pub method: SymbolId,
    pub name: String,
    pub param_tys: Vec<Type>,
    pub ret_ty: Type,
    pub raw_param_tys: Vec<Type>,
    pub raw_ret_ty: Type,
}

fn sam_excluded_name(name: &str) -> bool {
    matches!(
        name,
        "<init>"
            | "<clinit>"
            | "$init$"
            | "equals"
            | "hashCode"
            | "toString"
            | "clone"
            | "finalize"
            | "wait"
            | "notify"
            | "notifyAll"
            | "getClass"
            | "asInstanceOf"
            | "isInstanceOf"
            | "=="
            | "!="
            | "eq"
            | "ne"
            | "##"
            | "synchronized"
    )
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

fn subst_map(ty: &Type, tps: &[scala_rs_parser::SymbolId], args: &[Type]) -> Type {
    match ty {
        Type::TypeParam(id) => tps
            .iter()
            .position(|t| t == id)
            .and_then(|i| args.get(i).cloned())
            .unwrap_or_else(|| ty.clone()),
        Type::TypeMember(_) => ty.clone(),
        Type::Class { sym, args: as_ } => Type::Class {
            sym: *sym,
            args: as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        },
        Type::Applied { ctor, args: as_ } => apply_type_ctor(
            subst_map(ctor, tps, args),
            as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        ),
        Type::Array(t) => Type::Array(Box::new(subst_map(t, tps, args))),
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| subst_map(p, tps, args)).collect(),
            ret: Box::new(subst_map(ret, tps, args)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| subst_map(p, tps, args)).collect())
                .collect(),
            ret: Box::new(subst_map(ret, tps, args)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_map(t, tps, args))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_map(t, tps, args))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|t| subst_map(t, tps, args)).collect()),
        Type::Named { name, args: as_ } => Type::Named {
            name: name.clone(),
            args: as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(|p| subst_map(p, tps, args)).collect(),
            decls: decls
                .iter()
                .map(|d| subst_refine_decl(d, tps, args))
                .collect(),
        },
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(subst_map(tpe, tps, args)),
            annot: annot.clone(),
        },
        Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
            lo: lo.as_ref().map(|t| Box::new(subst_map(t, tps, args))),
            hi: hi.as_ref().map(|t| Box::new(subst_map(t, tps, args))),
        },
        Type::SingleType { prefix, sym } => Type::SingleType {
            prefix: Box::new(subst_map(prefix, tps, args)),
            sym: *sym,
        },
        other => other.clone(),
    }
}

fn expand_refine_decl(st: &SymbolTable, from: SymbolId, d: &RefineDecl) -> RefineDecl {
    match d {
        RefineDecl::Type {
            name,
            rhs,
            tparams,
            lo,
            hi,
        } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(|t| st.expand_type_members(from, t)),
            tparams: *tparams,
            lo: lo.as_ref().map(|t| st.expand_type_members(from, t)),
            hi: hi.as_ref().map(|t| st.expand_type_members(from, t)),
        },
        RefineDecl::Def { name, paramss, ret } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| st.expand_type_members(from, p)).collect())
                .collect(),
            ret: st.expand_type_members(from, ret),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: st.expand_type_members(from, ty),
        },
    }
}

fn subst_refine_decl(
    d: &RefineDecl,
    tps: &[scala_rs_parser::SymbolId],
    args: &[Type],
) -> RefineDecl {
    match d {
        RefineDecl::Type {
            name,
            rhs,
            tparams,
            lo,
            hi,
        } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(|t| subst_map(t, tps, args)),
            tparams: *tparams,
            lo: lo.as_ref().map(|t| subst_map(t, tps, args)),
            hi: hi.as_ref().map(|t| subst_map(t, tps, args)),
        },
        RefineDecl::Def { name, paramss, ret } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| subst_map(p, tps, args)).collect())
                .collect(),
            ret: subst_map(ret, tps, args),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: subst_map(ty, tps, args),
        },
    }
}

fn subst_refine_aliases(st: &SymbolTable, decls: &[RefineDecl], ty: &Type) -> Type {
    match ty {
        Type::TypeMember(id) => {
            let name = st.get(*id).name.clone();
            for d in decls {
                if let RefineDecl::Type {
                    name: n,
                    rhs: Some(rhs),
                    ..
                } = d
                {
                    if n == &name {
                        // Placeholder `TypeMember` rhs is the refinement's own
                        // member (`{ type F[X] = Id[X] }` stores `TypeMember(id)`).
                        // Recursing would match the same decl forever.
                        return match rhs {
                            Type::TypeMember(_) => rhs.clone(),
                            _ => subst_refine_aliases(st, decls, rhs),
                        };
                    }
                }
            }
            ty.clone()
        }
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args
                .iter()
                .map(|a| subst_refine_aliases(st, decls, a))
                .collect(),
        },
        Type::Applied { ctor, args } => {
            let applied = apply_type_ctor(
                subst_refine_aliases(st, decls, ctor),
                args.iter()
                    .map(|a| subst_refine_aliases(st, decls, a))
                    .collect(),
            );
            st.expand_applied_hk_alias(applied)
        }
        Type::Array(t) => Type::Array(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Function { params, ret } => Type::Function {
            params: params
                .iter()
                .map(|p| subst_refine_aliases(st, decls, p))
                .collect(),
            ret: Box::new(subst_refine_aliases(st, decls, ret)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| {
                    ps.iter()
                        .map(|p| subst_refine_aliases(st, decls, p))
                        .collect()
                })
                .collect(),
            ret: Box::new(subst_refine_aliases(st, decls, ret)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Tuple(ts) => Type::Tuple(
            ts.iter()
                .map(|t| subst_refine_aliases(st, decls, t))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub(crate) fn subst_tparams_slice(tps: &[SymbolId], args: &[Type], ty: &Type) -> Type {
    subst_map(ty, tps, args)
}

/// Apply type arguments to a constructor (`Id` + `[A]` → `Id[A]`).
pub fn apply_type_ctor(ctor: Type, args: Vec<Type>) -> Type {
    if args.is_empty() {
        return ctor;
    }
    match ctor {
        Type::Class {
            sym,
            args: existing,
        } => {
            let mut all = existing;
            all.extend(args);
            Type::Class { sym, args: all }
        }
        Type::Named {
            name,
            args: existing,
        } => {
            let mut all = existing;
            all.extend(args);
            Type::Named { name, args: all }
        }
        Type::Applied {
            ctor,
            args: existing,
        } => {
            let mut all = existing;
            all.extend(args);
            apply_type_ctor(*ctor, all)
        }
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(apply_type_ctor(*tpe, args)),
            annot,
        },
        other => Type::Applied {
            ctor: Box::new(other),
            args,
        },
    }
}

/// A type argument that stands for a range rather than one type.
fn is_wildcard_arg(t: &Type) -> bool {
    matches!(t, Type::Wildcard | Type::BoundedWildcard { .. })
}
