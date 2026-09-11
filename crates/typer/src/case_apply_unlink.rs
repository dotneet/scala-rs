//! A companion's own `apply` suppresses a case class's synthetic one.
//!
//! nsc (`Namers.caseApplyMeth`'s completer) unlinks the synthetic `apply`, and
//! the `apply$default$n` getters made for it, when the companion has a
//! concrete `apply` -- declared or inherited (`memberBasedOnName`) -- whose
//! signature matches. What is left is an ordinary method with ordinary
//! defaults:
//!
//! ```scala
//! case class C(x: Int = 1)
//! object C { def apply(x: Int = 2) = new C(x) }
//! C().x   // 2 (run/t10389)
//!
//! trait Companion[T] { def apply(value: String): T = parse(value).get; … }
//! object D extends Companion[D] { … }
//! case class D(value: String)
//! D("")   // Companion.apply, which validates (run/t10261)
//! ```
//!
//! scala-rs kept both `apply`s in the companion's scope. `C()` resolved to the
//! synthetic one and spliced the *constructor's* default, and the getter the
//! companion emitted was the constructor's too, because `ctor_defaults` had
//! already claimed the name `apply$default$1`.
//!
//! An `apply` with a different signature is an overload and leaves the
//! synthetic one in place, as in nsc; so does an abstract one, which the
//! synthetic method implements (`object P extends (Int => P)`).

use crate::check::Typer;
use crate::symbol::SymKind;
use scala_rs_parser::ast::Type;
use scala_rs_parser::{Flags, SymbolId};

impl Typer {
    /// A written `apply` of the module class `owner`, its signature just
    /// typed and its own default getters not yet declared.
    pub(crate) fn unlink_suppressed_case_apply(
        &mut self,
        owner: SymbolId,
        user_apply: SymbolId,
        user_tparams: &[SymbolId],
        user_paramss: &[Vec<Type>],
    ) {
        if user_apply.is_none()
            || self.st.get(user_apply).flags.contains(Flags::SYNTHETIC)
            || self.st.get(user_apply).flags.contains(Flags::ABSTRACT)
        {
            return;
        }
        let flat: Vec<Type> = user_paramss.iter().flatten().cloned().collect();
        self.unlink_case_apply_matching(owner, user_tparams, &flat);
    }

    /// The `apply`s the module class `owner` inherits, once its parents are
    /// known. Each is read as seen from the module: `Companion[D]`'s
    /// `apply(value: T)` takes a `D`.
    pub(crate) fn unlink_case_apply_by_inherited(&mut self, owner: SymbolId) {
        if owner.is_none() || self.st.get(owner).kind != SymKind::ModuleClass {
            return;
        }
        if self.synthetic_case_apply(owner).is_none() {
            return;
        }
        let this_ty = self.st.type_of_class(owner);
        let bases = self.st.base_type_seq(&this_ty);
        // A parent read from a classfile declares its members lazily, from
        // its pickle: ask for `apply` before looking it up, and complete the
        // signature of what comes back.
        for b in &bases {
            if let Some(c) = self.st.class_sym_of(b) {
                if c != owner {
                    self.supply_from_pickle_class(c, "apply");
                }
            }
        }
        let found = self.st.lookup_member(owner, "apply");
        for &m in &found {
            if self.st.get(m).owner != owner {
                self.complete_lazy_sig(m, scala_rs_span::Span::new(0, 0));
            }
        }
        for m in found {
            let s = self.st.get(m);
            if s.owner == owner
                || s.kind != SymKind::Method
                || self.st.method_is_deferred(m)
                || s.flags.contains(Flags::SYNTHETIC)
            {
                continue;
            }
            let Type::Method { paramss, .. } = s.ty.clone() else {
                continue;
            };
            let mowner = s.owner;
            let tparams = s.tparams.clone();
            let args = bases.iter().find_map(|b| match b {
                Type::Class { sym, args } if *sym == mowner => Some(args.clone()),
                _ => None,
            });
            let flat: Vec<Type> = paramss
                .iter()
                .flatten()
                .map(|t| match &args {
                    Some(a) if !a.is_empty() => self.st.subst_tparams(mowner, a, t),
                    _ => t.clone(),
                })
                .collect();
            if self.unlink_case_apply_matching(owner, &tparams, &flat) {
                return;
            }
        }
    }

    fn synthetic_case_apply(&self, owner: SymbolId) -> Option<SymbolId> {
        self.st.get(owner).members.iter().copied().find(|&m| {
            let s = self.st.get(m);
            s.name == "apply" && s.flags.contains(Flags::SYNTHETIC) && s.flags.contains(Flags::CASE)
        })
    }

    /// Unlink `owner`'s synthetic case `apply` if an `apply` taking
    /// `params` (with type parameters `tparams`) matches its signature.
    fn unlink_case_apply_matching(
        &mut self,
        owner: SymbolId,
        tparams: &[SymbolId],
        params: &[Type],
    ) -> bool {
        if owner.is_none() || self.st.get(owner).kind != SymKind::ModuleClass {
            return false;
        }
        let Some(synthetic) = self.synthetic_case_apply(owner) else {
            return false;
        };
        let Some(class_id) = self.linked_case_class(owner) else {
            return false;
        };
        // The synthetic `apply` takes the primary constructor's parameters;
        // their types are known from the header pass on, whether or not the
        // class's own signature pass has filled in the method type yet.
        let ctor_tys: Vec<Type> = self
            .st
            .get(synthetic)
            .params
            .iter()
            .map(|p| self.st.get(*p).ty.clone())
            .collect();
        if ctor_tys.iter().any(|t| t.is_no_type() || t.is_error()) || params.len() != ctor_tys.len()
        {
            return false;
        }
        let class_tps = self.st.get(class_id).tparams.clone();
        if tparams.len() != class_tps.len() {
            return false;
        }
        let as_class: Vec<Type> = class_tps.iter().map(|t| Type::TypeParam(*t)).collect();
        let matches = params.iter().zip(ctor_tys.iter()).all(|(u, c)| {
            let u = crate::symbol::subst_tparams_slice(tparams, &as_class, u);
            self.st.is_sub_type(&u, c) && self.st.is_sub_type(c, &u)
        });
        if !matches {
            return false;
        }
        // The getters `ctor_defaults` declared for the synthetic `apply`. A
        // written `apply`'s own are declared right after this returns, under
        // the same names.
        let getters: Vec<SymbolId> = self
            .st
            .get(owner)
            .members
            .iter()
            .copied()
            .filter(|&m| {
                let s = self.st.get(m);
                s.name.starts_with("apply$default$") && s.flags.contains(Flags::SYNTHETIC)
            })
            .collect();
        let gone = |m: &SymbolId| *m != synthetic && !getters.contains(m);
        self.st.get_mut(owner).members.retain(gone);
        // The module *value* carries the synthetic `apply` as well
        // (`synthesize_case_members`), for `C(1)` read off the term.
        if let Some(m) = self.st.companion_module(class_id).filter(|m| *m != owner) {
            self.st.get_mut(m).members.retain(gone);
        }
        true
    }

    /// The case class a module class is the companion of.
    fn linked_case_class(&self, module_cls: SymbolId) -> Option<SymbolId> {
        let s = self.st.get(module_cls);
        let base = s.name.strip_suffix('$').unwrap_or(&s.name).to_string();
        let owner = s.owner;
        self.st.get(owner).members.iter().copied().find(|&c| {
            let cs = self.st.get(c);
            cs.kind == SymKind::Class && cs.name == base && cs.flags.contains(Flags::CASE)
        })
    }
}
