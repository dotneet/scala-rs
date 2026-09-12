//! Type prefixes for inner classes: nsc's `TypeRef(pre, sym, args)` for the
//! one place `Type::Class { sym, args }` cannot express it.
//!
//! An inner class of a class -- `class Outer[T] { class In }` -- names one
//! type per *enclosing instance*: `a.In` and `b.In` are different types for
//! two stable `a`, `b: Outer`, `Outer#In` is the supertype of all of them,
//! and the outer type parameters a member of `In` mentions (`def t: T`) are
//! instantiated by the prefix (`a.In` with `a: Outer[String]` has `t:
//! String`). `Type::Class` carries neither, so the prefix rides on the
//! *as-seen-from view* that `A#B` projections already use (a `Type::Refined`
//! marked with [`AS_SEEN_FROM_MARK`]), as one more bookkeeping decl named
//! [`PREFIX_MARK`] whose right-hand side is the prefix type:
//!
//! * `ThisType(C)` for a bare `In` written inside `C` (`C.this.In`), for a
//!   `this.In`, or for the `C.this` an override check reads members through;
//! * `SingleType { .. }` / `ModuleRef` for `p.In` on a stable path;
//! * any other type for a projection `P#In`, or for a member read through an
//!   unstable receiver of that type (`unstable.In`, which nsc skolemizes; here
//!   it is the projection, which accepts strictly more and never less).
//!
//! A `Type::Class` for an inner class with *no* view is an unknown prefix --
//! every jar signature, every pickled type and every type this compiler
//! built before this module arrives that way -- and conforms exactly as it
//! did before: prefixes are only compared when both sides carry one.
//!
//! Everything downstream of the typer keeps seeing the class: `as_seen_from_view`
//! strips the view for erasure, the pickle and the JVM descriptor.

use crate::symbol::{SymKind, SymbolTable, AS_SEEN_FROM_MARK};
use scala_rs_parser::{Flags, RefineDecl, SymbolId, Type};

/// The bookkeeping decl carrying a view's prefix. Never a name a program can
/// write, like `AS_SEEN_FROM_MARK`.
pub const PREFIX_MARK: &str = "<prefix>";

/// The prefix a view carries, if any.
pub fn view_prefix(ty: &Type) -> Option<&Type> {
    let Type::Refined { decls, .. } = ty else {
        return None;
    };
    SymbolTable::as_seen_from_view(ty)?;
    decls.iter().find_map(|d| match d {
        RefineDecl::Type { name, rhs, .. } if name == PREFIX_MARK => rhs.as_ref(),
        _ => None,
    })
}

/// The class type under a view, or `ty` itself.
pub fn strip_view(ty: &Type) -> &Type {
    SymbolTable::as_seen_from_view(ty).unwrap_or(ty)
}

/// `core` seen through `pre`. `core` is a class type or a view of one (whose
/// earlier prefix, if any, is replaced); anything else is handed back
/// untouched, since only a class type has a prefix to record.
pub fn with_prefix(core: Type, pre: Type) -> Type {
    let pre_decl = RefineDecl::Type {
        name: PREFIX_MARK.to_string(),
        rhs: Some(pre),
        tparams: 0,
        lo: None,
        hi: None,
    };
    match core {
        Type::Refined { parents, mut decls } if SymbolTable::as_seen_from_view_decls(&decls) => {
            decls.retain(|d| !matches!(d, RefineDecl::Type { name, .. } if name == PREFIX_MARK));
            decls.push(pre_decl);
            Type::Refined { parents, decls }
        }
        Type::Class { .. } => Type::Refined {
            parents: vec![core],
            decls: vec![
                RefineDecl::Type {
                    name: AS_SEEN_FROM_MARK.to_string(),
                    rhs: None,
                    tparams: 0,
                    lo: None,
                    hi: None,
                },
                pre_decl,
            ],
        },
        other => other,
    }
}

/// `with_prefix` when there is a prefix to put back.
pub fn with_prefix_opt(core: Type, pre: Option<&Type>) -> Type {
    match pre {
        Some(p) => with_prefix(core, p.clone()),
        None => core,
    }
}

/// `ty` with its prefix (and the rest of its view) removed, as the parents
/// of a class are stored: `base_type_args`, `linearize` and every other
/// reader of `parents` matches `Type::Class` directly.
pub fn parent_form(ty: &Type) -> Type {
    if view_prefix(ty).is_some() {
        strip_view(ty).clone()
    } else {
        ty.clone()
    }
}

impl SymbolTable {
    pub(crate) fn as_seen_from_view_decls(decls: &[RefineDecl]) -> bool {
        decls
            .iter()
            .any(|d| matches!(d, RefineDecl::Type { name, .. } if name == AS_SEEN_FROM_MARK))
    }

    /// Does `sym` name a class whose type depends on an enclosing instance:
    /// a class or trait declared directly inside a class or trait? A member
    /// of an object has exactly one enclosing instance, a local class has
    /// none, a Java nested class is static, and a module class is a value.
    pub fn is_inner_class_of_class(&self, sym: SymbolId) -> bool {
        if sym.is_none() {
            return false;
        }
        let s = self.get(sym);
        if s.kind != SymKind::Class {
            return false;
        }
        if s.flags.contains(Flags::JAVA) {
            return self.is_binary_nested_class(sym);
        }
        let owner = s.owner;
        if owner.is_none() || owner == sym {
            return false;
        }
        let o = self.get(owner);
        o.kind == SymKind::Class && !o.flags.contains(Flags::JAVA)
    }

    /// A class read from a class file (`Flags::JAVA` marks every binary
    /// symbol, Scala or Java) that is nested in a class -- `Numeric$NumericOps`,
    /// `RelationalProfile$Table` -- and not `static`: the `InnerClasses`
    /// attribute says which (`nested_static`, `classpath.rs`), and a class
    /// nested in an object is static. Such a class takes an enclosing
    /// instance exactly like one from source.
    pub fn is_binary_nested_class(&self, sym: SymbolId) -> bool {
        let s = self.get(sym);
        if !self.binary_read.contains(&sym.0)
            || s.kind != SymKind::Class
            || s.flags.contains(Flags::STATIC)
            || s.flags.contains(Flags::MODULE)
            || s.owner.is_none()
            || s.owner == sym
        {
            return false;
        }
        let simple = s.jvm_name.rsplit('/').next().unwrap_or("");
        if !simple.contains('$') || simple.ends_with('$') {
            return false;
        }
        self.get(s.owner).kind == SymKind::Class
    }

    /// Does `ty` name a class `is_inner_class_of_class` says yes to, with no
    /// prefix recorded yet?
    pub fn is_bare_inner_class_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Class { sym, .. } if self.is_inner_class_of_class(*sym))
    }

    /// The prefix form of a receiver: a class applied to its *own* type
    /// parameters is the class's `this` (that is what `class_this_ty` and
    /// `self_type_of_class` build), and nsc writes it `C.this`; a singleton
    /// is itself. Any other class type is a *widened* receiver -- a value
    /// this compiler did not keep the path of. nsc skolemizes it (`_1.In
    /// forSome { val _1: C[X] }`); here it is the type itself, which reads
    /// as the projection `C[X]#In`: its arguments still instantiate the
    /// enclosing class (`new SB[F] |@| fa` in cats' semigroupal syntax), and
    /// it is no particular instance, so it is not an `a.In` -- the same
    /// answer nsc gives. A class with no parameters and no arguments is
    /// read as its `this`: nothing distinguishes the two spellings, and
    /// `expand_in_type` reaches here with the class's own type.
    pub fn canonical_prefix(&self, recv: &Type) -> Type {
        match recv {
            Type::Class { sym, args } if !sym.is_none() => {
                let tps = &self.get(*sym).tparams;
                let own = tps.len() == args.len()
                    && tps
                        .iter()
                        .zip(args)
                        .all(|(tp, a)| matches!(a, Type::TypeParam(id) if id == tp));
                if own {
                    Type::ThisType(*sym)
                } else {
                    recv.clone()
                }
            }
            Type::ThisType(_) | Type::SingleType { .. } | Type::ModuleRef(_) => recv.clone(),
            // An inner class behind a prefix is itself a prefix for the
            // classes nested in it: `o.mid.in` for `class Mid { def in = new
            // In }` is an `o.mid.In`-like `Mid`-with-prefix-`o` instance.
            Type::Refined { .. } if view_prefix(recv).is_some() => recv.clone(),
            _ => Type::NoType,
        }
    }

    /// A prefix that denotes one value: `this`, a stable path, or an object.
    pub fn is_singleton_prefix(&self, pre: &Type) -> bool {
        matches!(
            pre,
            Type::ThisType(_) | Type::SingleType { .. } | Type::ModuleRef(_)
        )
    }

    /// `ThisType(c)` for a self alias `self` of `c`; the singleton otherwise.
    fn norm_singleton(&self, pre: &Type) -> Type {
        match pre {
            // An object is one value however it is spelled: `O.type`,
            // `O.In`'s `ModuleRef`, or `this` inside `O`.
            Type::SingleType { sym, .. }
                if !sym.is_none()
                    && matches!(self.get(*sym).kind, SymKind::Module | SymKind::ModuleClass) =>
            {
                Type::ThisType(self.module_class_of(*sym))
            }
            Type::SingleType { sym, .. } if !sym.is_none() => {
                let owner = self.get(*sym).owner;
                if !owner.is_none() && self.get(owner).self_alias == Some(*sym) {
                    Type::ThisType(owner)
                } else {
                    pre.clone()
                }
            }
            Type::ModuleRef(m) if !m.is_none() => Type::ThisType(self.module_class_of(*m)),
            other => other.clone(),
        }
    }

    /// Are two singleton prefixes the same value?
    ///
    /// Deliberately lenient where this compiler cannot tell: two `this`
    /// prefixes of classes related by inheritance are the same (the override
    /// checker reads both members through the subclass, where nsc's
    /// as-seen-from has already rewritten `P.this` to `P2.this`), and a
    /// `SingleType` whose own prefix was rewritten to a class type (a member
    /// read through an unstable receiver) compares by its symbol alone.
    pub fn same_singleton_prefix(&self, a: &Type, b: &Type) -> bool {
        let a = self.norm_singleton(a);
        let b = self.norm_singleton(b);
        match (&a, &b) {
            (Type::ThisType(x), Type::ThisType(y)) => {
                x == y || self.is_ancestor_of(*x, *y) || self.is_ancestor_of(*y, *x)
            }
            (
                Type::SingleType {
                    prefix: p1,
                    sym: s1,
                },
                Type::SingleType {
                    prefix: p2,
                    sym: s2,
                },
            ) => {
                // Dependent method types: `def foo(a: A)(v: a.V)` overridden
                // by `def foo(a: A)(v: a.V)` names two different `a`s, which
                // nsc matches positionally. Two method parameters of one name
                // are read as the same one (`run/t6135`).
                if s1 != s2 {
                    let (x, y) = (self.get(*s1), self.get(*s2));
                    let both_params = x.flags.contains(Flags::PARAM)
                        && y.flags.contains(Flags::PARAM)
                        && x.name == y.name
                        && !x.owner.is_none()
                        && !y.owner.is_none()
                        && self.get(x.owner).kind == SymKind::Method
                        && self.get(y.owner).kind == SymKind::Method;
                    if !both_params {
                        return false;
                    }
                }
                match (p1.as_ref(), p2.as_ref()) {
                    (Type::NoType, _) | (_, Type::NoType) => true,
                    (x, y) if self.is_singleton_prefix(x) && self.is_singleton_prefix(y) => {
                        self.same_singleton_prefix(x, y)
                    }
                    // One side's prefix is a class type: the path was read
                    // through an unstable receiver. Nothing to compare.
                    _ => true,
                }
            }
            _ => false,
        }
    }

    /// The type a prefix's values have: what `p.In <: P#In` compares `p`
    /// against.
    pub fn widen_prefix(&self, pre: &Type) -> Type {
        match pre {
            Type::ThisType(c) if !c.is_none() => self.self_type_of_class(*c),
            Type::SingleType { prefix, sym } if !sym.is_none() => {
                let t = self.singleton_underlying(*sym);
                if t.is_no_type() || matches!(t, Type::Method { .. }) {
                    self.widen_prefix(prefix)
                } else {
                    t
                }
            }
            other => other.clone(),
        }
    }

    /// nsc's `isSubPre`: does the prefix `a` conform to the prefix `b`?
    ///
    /// A singleton on the right admits only the same singleton; anything
    /// else is a projection over a type, which every prefix of a conforming
    /// type is in.
    pub fn prefix_conforms(&self, a: &Type, b: &Type) -> bool {
        if a == b || a.is_no_type() || b.is_no_type() {
            return true;
        }
        if self.is_singleton_prefix(b) {
            if !self.is_singleton_prefix(a) {
                return false;
            }
            if self.same_singleton_prefix(a, b) {
                return true;
            }
            // `p.X` against `P.this.X` where `p` is a `P`: a member declared
            // bare in `P` and read through `p` means `p.X` in nsc, and this
            // compiler does not rewrite every `P.this` on every road a
            // member type travels (an alias expanded after the selection,
            // say). A value of the class is accepted as that class's `this`;
            // two *different values* (`a.In` against `b.In`) and a value
            // against a path the required type names (`P2.this.S1` against
            // `p.S1`, neg/abstract-class-2) stay apart.
            if let Type::ThisType(c) = self.norm_singleton(b) {
                if !c.is_none() && matches!(a, Type::SingleType { .. } | Type::ModuleRef(_)) {
                    let wa = self.widen_prefix(a);
                    return !wa.is_no_type()
                        && !wa.is_error()
                        && self.is_sub_type(&wa, &self.self_type_of_class(c));
                }
            }
            return false;
        }
        let wa = self.widen_prefix(a);
        // A prefix that names no class (a type parameter left as a
        // projection prefix, `NoType` from a path this compiler could not
        // model) decides nothing.
        if wa.is_no_type() || wa.is_error() || b.is_no_type() || b.is_error() {
            return true;
        }
        self.is_sub_type(&wa, b)
    }

    /// Two prefixes that no instantiation can make the same: both are
    /// singletons and they are not the same one. Used by the override
    /// checker, which only rejects on certainty.
    pub fn prefixes_certainly_differ(&self, a: &Type, b: &Type) -> bool {
        self.is_singleton_prefix(a)
            && self.is_singleton_prefix(b)
            && !self.same_singleton_prefix(a, b)
    }

    /// Does the class type `core` (of an inner class), seen through `pre`,
    /// conform to `b` through one of its parents? The parents are declared
    /// in the enclosing class's vocabulary, and `pre` is what instantiates
    /// it: `class MapOps[K] { class KeySet extends MySet[K] }` gives
    /// `hm.KeySet <: MySet[Int]` for `hm: HM[Int]`.
    pub(crate) fn prefixed_parents_conform(&self, pre: &Type, core: &Type, b: &Type) -> bool {
        let Type::Class { sym, args } = core else {
            return false;
        };
        if sym.is_none() {
            return false;
        }
        let Some(_g) = crate::symbol::enter_parent_depth() else {
            return false;
        };
        let parents = self.get(*sym).parents.clone();
        parents.iter().any(|p| {
            let p = self.subst_tparams_cow(*sym, args, p);
            let p = self.subst_as_seen_from(pre, &p);
            self.is_sub_type(&p, b)
        })
    }

    /// Does `ty` mention an inner class of a class, bare or behind a prefix?
    /// The cheap test for whether a selection has any prefix work to do.
    pub fn mentions_inner_class(&self, ty: &Type) -> bool {
        crate::symbol::any_type(ty, &mut |t| {
            self.is_bare_inner_class_type(t) || view_prefix(t).is_some()
        })
    }

    /// nsc's as-seen-from for the `C.this` written in a view's prefix, for
    /// a member read through `recv` (selected on the stable path
    /// `inner_pre`, when there is one):
    ///
    /// * `C.this` for the receiver's own class or one of its ancestors is the
    ///   receiver -- `o.mk` for `def mk: In` (`Outer.this.In`) is an `o.In`;
    /// * `C.this` for a class *enclosing* the receiver's class is the
    ///   receiver path's own prefix at that depth -- `p.O.f` for `object O {
    ///   def f(x: S1) }` inside `P` takes a `p.S1` -- and, when the path does
    ///   not reach that far, an unknown prefix (`NoType`), which conforms to
    ///   anything: this compiler cannot tell which instance, so it does not
    ///   guess one.
    ///
    /// Only prefixes (of views and of the paths inside them) are rewritten;
    /// a `this.type` standing on its own keeps the treatment
    /// `subst_receiver_this_type` gives it.
    pub(crate) fn rewrite_view_this(
        &self,
        recv: &Type,
        inner_pre: Option<&Type>,
        ty: &Type,
    ) -> Type {
        let mut owners = Vec::new();
        Self::this_type_owners(ty, &mut owners);
        if owners.is_empty() {
            return ty.clone();
        }
        let Some(rc) = self.class_sym_of(recv) else {
            return ty.clone();
        };
        let pre_to = match inner_pre {
            Some(p) => p.clone(),
            None => self.canonical_prefix(recv),
        };
        let encl: Vec<SymbolId> = self
            .enclosing_classes(rc)
            .into_iter()
            .skip(1)
            .filter(|c| self.get(*c).is_class_like())
            .collect();
        // The enclosing instance `level` steps out of the receiver. It is
        // the receiver *type*'s own prefix (`database: JdbcBackend.this.
        // JdbcDatabaseDef[F]` has `JdbcBackend.this` around it, whatever
        // path `database` was reached by), then that prefix's, and so on;
        // for an object nested in a class, selected through a path, the
        // path's prefix (`p.O` is level 1 `p`). Through the receiver's own
        // `this`, an enclosing `E.this` is still `E.this` (`None`: leave
        // it). Past what is known: unknown.
        let outer_at = |level: usize| -> Option<Type> {
            let mut cur: Option<Type> = Some(recv.clone());
            for _ in 0..level {
                cur = match cur {
                    Some(ref t) => match view_prefix(t) {
                        Some(p) => Some(p.clone()),
                        None => match (t, inner_pre) {
                            (Type::ThisType(_), _) => return None,
                            (Type::Class { .. }, Some(Type::ThisType(_))) => return None,
                            (_, Some(Type::SingleType { prefix, sym }))
                                if self.is_singleton_prefix(prefix)
                                    && Some(*sym) == self.class_sym_of(t)
                                    && self.get(*sym).kind == SymKind::ModuleClass =>
                            {
                                Some((**prefix).clone())
                            }
                            (Type::SingleType { sym, .. }, _) if !sym.is_none() => {
                                let under = self.singleton_underlying(*sym);
                                view_prefix(&under).cloned()
                            }
                            _ => None,
                        },
                    },
                    None => None,
                };
            }
            Some(cur.unwrap_or(Type::NoType))
        };
        let target = |c: SymbolId| -> Option<Type> {
            if c == rc || self.is_ancestor_of(c, rc) {
                return Some(pre_to.clone());
            }
            let level = encl
                .iter()
                .position(|e| *e == c || self.is_ancestor_of(c, *e))?;
            outer_at(level + 1)
        };
        let mut hit = false;
        let mut plan: Vec<(SymbolId, Type)> = Vec::new();
        for c in owners {
            if let Some(t) = target(c) {
                hit = true;
                plan.push((c, t));
            }
        }
        if !hit {
            return ty.clone();
        }
        let f = |pre: &Type| -> Type {
            crate::symbol::map_type(pre, &mut |t| match t {
                Type::ThisType(c) => match plan.iter().find(|(x, _)| x == c) {
                    Some((_, to)) => to.clone(),
                    None => t.clone(),
                },
                other => other.clone(),
            })
        };
        map_view_prefixes(ty, &f)
    }

    /// Give every bare inner class of a class in `owners` the prefix `pre`,
    /// throughout `ty`. This is the half of as-seen-from that `subst_tparams`
    /// cannot do: a member of `Outer` that mentions `In` means
    /// `Outer.this.In`, and read through a receiver it is the receiver's
    /// `In`.
    pub(crate) fn attach_inner_prefixes(
        &self,
        owners: &rustc_hash::FxHashSet<u32>,
        pre: &Type,
        ty: Type,
    ) -> Type {
        // Nothing to say about the prefix: the class stays bare, which means
        // the same thing and costs nothing downstream.
        if pre.is_no_type() {
            return ty;
        }
        let wants = |st: &SymbolTable, sym: SymbolId| -> bool {
            st.is_inner_class_of_class(sym) && owners.contains(&st.get(sym).owner.0)
        };
        if !crate::symbol::any_type(
            &ty,
            &mut |t| matches!(t, Type::Class { sym, .. } if wants(self, *sym)),
        ) {
            return ty;
        }
        let on_class = |c: Type| -> Type {
            match &c {
                Type::Class { sym, .. } if wants(self, *sym) => with_prefix(c, pre.clone()),
                _ => c,
            }
        };
        map_views(&ty, &on_class, &|p| p.clone())
    }
}

/// The recursion of [`SymbolTable::attach_inner_prefixes`]: `on_class` sees
/// every class type that is not already under a view, and `on_prefix` every
/// view's prefix. A class already under a view keeps the prefix it has (its
/// arguments are still visited); a prefix is written in the caller's
/// vocabulary, not the member's, so `on_class` never runs inside one.
fn map_views(
    ty: &Type,
    on_class: &dyn Fn(Type) -> Type,
    on_prefix: &dyn Fn(&Type) -> Type,
) -> Type {
    let go = |t: &Type| map_views(t, on_class, on_prefix);
    match ty {
        Type::Class { sym, args } => on_class(Type::Class {
            sym: *sym,
            args: args.iter().map(go).collect(),
        }),
        Type::Refined { parents, decls } if SymbolTable::as_seen_from_view(ty).is_some() => {
            let parents = parents
                .iter()
                .map(|p| match p {
                    Type::Class { sym, args } => Type::Class {
                        sym: *sym,
                        args: args.iter().map(go).collect(),
                    },
                    other => go(other),
                })
                .collect();
            let decls = decls
                .iter()
                .map(|d| match d {
                    RefineDecl::Type {
                        name,
                        rhs: Some(pre),
                        tparams,
                        lo,
                        hi,
                    } if name == PREFIX_MARK => RefineDecl::Type {
                        name: name.clone(),
                        rhs: Some(on_prefix(pre)),
                        tparams: *tparams,
                        lo: lo.clone(),
                        hi: hi.clone(),
                    },
                    other => map_decl(other, &go),
                })
                .collect();
            Type::Refined { parents, decls }
        }
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(go).collect(),
            decls: decls.iter().map(|d| map_decl(d, &go)).collect(),
        },
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(go).collect()),
        Type::Overload(ts) => Type::Overload(ts.iter().map(go).collect()),
        Type::Applied { ctor, args } => Type::Applied {
            ctor: Box::new(go(ctor)),
            args: args.iter().map(go).collect(),
        },
        Type::Array(t) => Type::Array(Box::new(go(t))),
        Type::ByName(t) => Type::ByName(Box::new(go(t))),
        Type::Repeated(t) => Type::Repeated(Box::new(go(t))),
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(go(tpe)),
            annot: annot.clone(),
        },
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(go).collect(),
            ret: Box::new(go(ret)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(go).collect())
                .collect(),
            ret: Box::new(go(ret)),
        },
        Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
            lo: lo.as_ref().map(|t| Box::new(go(t))),
            hi: hi.as_ref().map(|t| Box::new(go(t))),
        },
        Type::Named { name, args } => Type::Named {
            name: name.clone(),
            args: args.iter().map(go).collect(),
        },
        Type::SingleType { prefix, sym } => Type::SingleType {
            prefix: Box::new(go(prefix)),
            sym: *sym,
        },
        other => other.clone(),
    }
}

/// `f` applied to the prefix of every view in `ty`.
fn map_view_prefixes(ty: &Type, f: &dyn Fn(&Type) -> Type) -> Type {
    map_views(ty, &|c| c, f)
}

fn map_decl(d: &RefineDecl, go: &dyn Fn(&Type) -> Type) -> RefineDecl {
    match d {
        RefineDecl::Type {
            name,
            rhs,
            tparams,
            lo,
            hi,
        } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(go),
            tparams: *tparams,
            lo: lo.as_ref().map(go),
            hi: hi.as_ref().map(go),
        },
        RefineDecl::Def { name, paramss, ret } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(go).collect())
                .collect(),
            ret: go(ret),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: go(ty),
        },
    }
}
