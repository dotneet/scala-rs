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
    if SymbolTable::as_seen_from_view(ty).is_none() {
        return None;
    }
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
        if s.kind != SymKind::Class || s.flags.contains(Flags::JAVA) {
            return false;
        }
        let owner = s.owner;
        if owner.is_none() || owner == sym {
            return false;
        }
        let o = self.get(owner);
        o.kind == SymKind::Class && !o.flags.contains(Flags::JAVA)
    }

    /// Does `ty` name a class `is_inner_class_of_class` says yes to, with no
    /// prefix recorded yet?
    pub fn is_bare_inner_class_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Class { sym, .. } if self.is_inner_class_of_class(*sym))
    }

    /// The prefix form of a receiver: a class applied to its *own* type
    /// parameters is the class's `this` (that is what `class_this_ty` and
    /// `self_type_of_class` build), and nsc writes it `C.this`.
    pub fn canonical_prefix(&self, recv: &Type) -> Type {
        match recv {
            Type::Class { sym, args } if !sym.is_none() && !args.is_empty() => {
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
            other => other.clone(),
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
                if s1 != s2 {
                    return false;
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
        if a == b {
            return true;
        }
        if self.is_singleton_prefix(b) {
            return self.is_singleton_prefix(a) && self.same_singleton_prefix(a, b);
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
        let wants = |st: &SymbolTable, sym: SymbolId| -> bool {
            st.is_inner_class_of_class(sym) && owners.contains(&st.get(sym).owner.0)
        };
        if !crate::symbol::any_type(
            &ty,
            &mut |t| matches!(t, Type::Class { sym, .. } if wants(self, *sym)),
        ) {
            return ty;
        }
        attach_rec(self, &ty, &wants, pre)
    }
}

/// The recursion of [`SymbolTable::attach_inner_prefixes`]. A class already
/// under a view keeps the prefix it has; the view's arguments are still
/// visited.
fn attach_rec(
    st: &SymbolTable,
    ty: &Type,
    wants: &dyn Fn(&SymbolTable, SymbolId) -> bool,
    pre: &Type,
) -> Type {
    let go = |t: &Type| attach_rec(st, t, wants, pre);
    match ty {
        Type::Class { sym, args } => {
            let core = Type::Class {
                sym: *sym,
                args: args.iter().map(go).collect(),
            };
            if wants(st, *sym) {
                with_prefix(core, pre.clone())
            } else {
                core
            }
        }
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
                    // The prefix itself is left alone: it is written in the
                    // caller's vocabulary, not the member's.
                    RefineDecl::Type { name, .. } if name == PREFIX_MARK => d.clone(),
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
        other => other.clone(),
    }
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
