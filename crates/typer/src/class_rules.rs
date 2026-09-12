//! Class-level rules nsc enforces once a template's members are known, which
//! scala-rs used to let through.
//!
//! * **`new C` without arguments** for a class whose constructors all need
//!   some (`unapplied_new_error`).
//!
//! * **Defaults in more than one overloaded alternative** (nsc
//!   `RefChecks.checkDefaultsInOverloaded`): the default getters are named
//!   `f$default$N` whatever the alternative, so two alternatives of `f` that
//!   both define defaults would share getters.
//!
//!   ```scala
//!   object A {
//!     def apply(a: Int = 1): Int = a
//!     def apply(a: String, b: Int = 2): Int = b   // multiple overloaded alternatives ...
//!   }
//!   ```
//!
//!   scala/scala `neg/t3909`, `neg/t8157`. The check covers inherited members
//!   too (`class B extends A with T` with a default-taking `f` in each
//!   parent), and an overriding method inherits the defaults of the method it
//!   overrides. Constructors are `crate::ctor_defaults`'s.

use crate::check::Typer;
use crate::symbol::SymKind;
use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_span::Span;

impl Typer {
    /// `new C` written without an argument list, for a class whose every
    /// constructor needs arguments: nsc reads it as `new C()` and reports the
    /// missing ones.
    ///
    /// ```scala
    /// class X(x: Int)
    /// val a = new X   // not enough arguments for constructor X: (x: Int): X.
    /// ```
    ///
    /// scala-rs accepted it and emitted a call to a `<init>()V` the class
    /// does not have (scala/scala `neg/t1038`). Only classes this run
    /// compiles are judged: their constructors are all known. A constructor
    /// taking a single parameter `()` could be adapted to (a type parameter,
    /// `Any`, `AnyVal`, `Unit`) is left alone, as nsc inserts the `()`.
    pub(crate) fn unapplied_new_error(&self, cls: SymbolId, targs: &[Type]) -> Option<String> {
        let s = self.st.get(cls);
        if s.kind != SymKind::Class
            || s.flags.contains(Flags::TRAIT)
            || s.flags.contains(Flags::ABSTRACT)
            || s.flags.contains(Flags::INTERFACE)
            || s.flags.contains(Flags::JAVA)
            || cls.0 < self.st.prelude_end
            || !s.pickled_origin.is_empty()
            || s.binary_outer_desc.is_some()
            || s.name.starts_with("$anon")
            || cls.0 < self.st.source_start
        {
            return None;
        }
        let ctors: Vec<SymbolId> = s
            .members
            .iter()
            .copied()
            .filter(|&m| self.st.get(m).kind == SymKind::Method && self.st.get(m).name == "<init>")
            .collect();
        if ctors.is_empty() {
            return None;
        }
        let mut shown = Vec::new();
        for &c in &ctors {
            let cs = self.st.get(c);
            // A constructor supplied from a pickle or read from a class file
            // states no implicit clause as such, so "every parameter is
            // omissible" cannot be read off it: `new UnrolledBuffer[Int]`,
            // whose only clause is an implicit `ClassTag`, looked like a call
            // missing its argument. The classfile reader names such parameters
            // `x$0`, `x$1`, … -- having the declaration's own names is exactly
            // the condition this rule (and nsc's "Unspecified value parameter
            // a") needs.
            if !cs.pickled_origin.is_empty() {
                return None;
            }
            let synthetic_names = cs.paramss.iter().flatten().any(|p| {
                let n = &self.st.get(*p).name;
                n.strip_prefix("x$").is_some_and(|rest| {
                    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
                })
            });
            if synthetic_names {
                return None;
            }
            let Type::Method { paramss, .. } = &cs.ty else {
                return None;
            };
            let first = paramss.first().cloned().unwrap_or_default();
            if first.is_empty() {
                return None;
            }
            let ids: Vec<SymbolId> = cs.paramss.first().cloned().unwrap_or_default();
            if ids.len() != first.len() {
                return None;
            }
            // `new Gen[Int]` states the argument: the parameter is `Int`, not
            // the declaration's `T`, so the `()`-insertion below must not read
            // it as an open type parameter.
            let first = self.at_written_targs(cls, targs, &first);
            let omissible = ids.iter().all(|p| {
                let f = self.st.get(*p).flags;
                f.contains(Flags::DEFAULTPARAM) || f.contains(Flags::IMPLICIT)
            });
            let unit_adaptable = first.len() == 1
                && matches!(
                    &first[0],
                    Type::TypeParam(_) | Type::Any | Type::AnyVal | Type::Unit
                );
            if omissible || unit_adaptable {
                return None;
            }
            shown.push((c, ids));
        }
        let class_ty = Type::Class {
            sym: cls,
            args: if targs.len() == s.tparams.len() && !targs.is_empty() {
                targs.to_vec()
            } else {
                s.tparams.iter().map(|t| Type::TypeParam(*t)).collect()
            },
        };
        let sig = |c: SymbolId| -> String {
            let cs = self.st.get(c);
            let Type::Method { paramss, .. } = &cs.ty else {
                return String::new();
            };
            let mut out = String::new();
            for (i, clause) in paramss.iter().enumerate() {
                let clause = &self.at_written_targs(cls, targs, clause);
                let ids = cs.paramss.get(i).cloned().unwrap_or_default();
                let implicit = ids
                    .first()
                    .is_some_and(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT));
                let parts: Vec<String> = clause
                    .iter()
                    .enumerate()
                    .map(|(j, t)| {
                        let n = ids
                            .get(j)
                            .map(|p| self.st.get(*p).name.clone())
                            .unwrap_or_else(|| format!("x${}", j + 1));
                        format!("{n}: {}", self.st.display_type(t))
                    })
                    .collect();
                out.push('(');
                if implicit {
                    out.push_str("implicit ");
                }
                out.push_str(&parts.join(", "));
                out.push(')');
            }
            out
        };
        let name = s.name.clone();
        let cls_shown = self.st.display_type(&class_ty);
        if let [(c, ids)] = shown.as_slice() {
            let names: Vec<String> = ids.iter().map(|p| self.st.get(*p).name.clone()).collect();
            let unspecified = if names.len() == 1 {
                format!("Unspecified value parameter {}.", names[0])
            } else {
                format!("Unspecified value parameters {}.", names.join(", "))
            };
            Some(format!(
                "not enough arguments for constructor {name}: {}: {cls_shown}.\n{unspecified}",
                sig(*c)
            ))
        } else {
            let alts: Vec<String> = shown
                .iter()
                .map(|(c, _)| format!("  {}{cls_shown}", sig(*c)))
                .collect();
            Some(format!(
                "multiple constructors for {name} with alternatives:\n{}\n cannot be invoked with no arguments",
                alts.join(" <and>\n")
            ))
        }
    }

    /// `tys` with the class's type parameters replaced by the type arguments
    /// the `new` wrote, when it wrote a full list.
    fn at_written_targs(&self, cls: SymbolId, targs: &[Type], tys: &[Type]) -> Vec<Type> {
        let tps = self.st.get(cls).tparams.clone();
        if targs.len() != tps.len() || targs.is_empty() {
            return tys.to_vec();
        }
        tys.iter()
            .map(|t| crate::symbol::subst_tparams_slice(&tps, targs, t))
            .collect()
    }

    /// nsc's namer: a parameter of a method that overrides or implements one
    /// whose parameter declares a default inherits that default -- the
    /// parameter is `DEFAULTPARAM` though it writes none, and a call that
    /// omits it invokes the inherited `name$default$n` getter on the receiver.
    ///
    /// ```scala
    /// class A { def g(a: Int = 1): Int = a }
    /// class C extends A { override def g(a: Int): Int = a + 100 }
    /// new C().g()   // 101: A.g$default$1 called on the C
    /// ```
    ///
    /// scala-rs saw a parameter without a default and reported "no matching
    /// overload ... with arguments ()". No getter is synthesized for such a
    /// parameter: the inherited one is the one nsc calls, and a class
    /// further down that writes its own default overrides it.
    ///
    /// Run once the signature pass has given every source method its
    /// parameter symbols and types, before any body (any call) is typed.
    pub(crate) fn inherit_overridden_defaults(&mut self) {
        let end = self.st.symbols.len();
        for idx in self.st.prelude_end as usize..end {
            let m = SymbolId(idx as u32);
            let s = self.st.get(m);
            if s.kind != SymKind::Method
                || s.name == "<init>"
                || s.name.contains("$default$")
                || !s.pickled_origin.is_empty()
                || s.owner.is_none()
                || !self.st.get(s.owner).is_class_like()
            {
                continue;
            }
            let params: Vec<SymbolId> = s.paramss.iter().flatten().copied().collect();
            if params.is_empty()
                || params
                    .iter()
                    .all(|p| self.st.get(*p).flags.contains(Flags::DEFAULTPARAM))
            {
                continue;
            }
            let (owner, name) = (s.owner, s.name.clone());
            let bases: Vec<SymbolId> = self
                .st
                .lookup_member(owner, &name)
                .into_iter()
                .filter(|&b| {
                    b != m
                        && self.st.get(b).owner != owner
                        && self.st.get(b).kind == SymKind::Method
                })
                .collect();
            for b in bases {
                let bs = self.st.get(b);
                let bparams: Vec<SymbolId> = if bs.paramss.is_empty() {
                    bs.params.clone()
                } else {
                    bs.paramss.iter().flatten().copied().collect()
                };
                if bparams.len() != params.len() || !self.same_signature(m, b) {
                    continue;
                }
                for (p, bp) in params.iter().zip(&bparams) {
                    if self.st.get(*bp).flags.contains(Flags::DEFAULTPARAM)
                        && !self.st.get(*p).flags.contains(Flags::DEFAULTPARAM)
                    {
                        let f = self.st.get(*p).flags.with(Flags::DEFAULTPARAM);
                        self.st.get_mut(*p).flags = f;
                    }
                }
            }
        }
    }

    /// Whether `m` is a method whose parameters declare a default.
    fn declares_default(&self, m: SymbolId) -> bool {
        let s = self.st.get(m);
        s.kind == SymKind::Method
            && s.name != "<init>"
            && s.params
                .iter()
                .chain(s.paramss.iter().flatten())
                .any(|p| self.st.get(*p).flags.contains(Flags::DEFAULTPARAM))
    }

    /// nsc `RefChecks.checkDefaultsInOverloaded`, for the class or module
    /// class `cls` written at `span`.
    pub(crate) fn check_default_overloads(&mut self, cls: SymbolId, span: Span) {
        if cls.is_none() {
            return;
        }
        let mut names: Vec<String> = Vec::new();
        for m in self.st.members_including_inherited(cls) {
            if self.declares_default(m) {
                let n = self.st.get(m).name.clone();
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        for name in names {
            let found = self.st.lookup_member(cls, &name);
            let kept = self.drop_overridden(found.clone());
            // A member counts when it declares a default itself or overrides
            // one that does: nsc copies `DEFAULTPARAM` onto the overriding
            // parameter.
            let with_defaults: Vec<SymbolId> = kept
                .iter()
                .copied()
                .filter(|&m| {
                    self.st.get(m).kind == SymKind::Method
                        && self.st.get(m).name != "<init>"
                        && (self.declares_default(m)
                            || found.iter().any(|&o| {
                                o != m
                                    && !kept.contains(&o)
                                    && self.declares_default(o)
                                    && self.same_signature(m, o)
                            }))
                })
                .collect();
            // Only what this run's sources or a Java class file declare: the
            // prelude's hand-written stand-ins and pickled library members
            // can both carry a member twice.
            if with_defaults
                .iter()
                .any(|&m| m.0 < self.st.prelude_end || !self.st.get(m).pickled_origin.is_empty())
            {
                continue;
            }
            // nsc walks the members in order; each one with defaults is
            // checked against the later ones, a concrete one ignoring a later
            // deferred one.
            for (i, &x) in with_defaults.iter().enumerate() {
                let x_deferred = self.st.method_is_deferred(x);
                let others: Vec<SymbolId> = with_defaults[i + 1..]
                    .iter()
                    .copied()
                    .filter(|&alt| !self.st.method_is_deferred(alt) || x_deferred)
                    .collect();
                if others.is_empty() {
                    continue;
                }
                let mut all = vec![x];
                all.extend(others);
                let rest = if all.iter().any(|&m| self.st.get(m).owner != cls) {
                    let owners: Vec<String> = all
                        .iter()
                        .map(|&m| self.defining_owner_desc(self.st.get(m).owner))
                        .collect();
                    format!(
                        ".\nThe members with defaults are defined in {}.",
                        owners.join(" and ")
                    )
                } else {
                    ".".to_string()
                };
                let msg = format!(
                    "in {}, multiple overloaded alternatives of method {name} define default arguments{rest}",
                    self.defining_owner_desc(cls)
                );
                self.error(span, msg);
            }
        }
    }
}
