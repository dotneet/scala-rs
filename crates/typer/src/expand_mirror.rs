//! A class this run is compiling, as the macro engine's mirror sees it: in
//! nsc's shape, and only when it is asked (`docs/macros.md` §7.25).
//!
//! A macro implementation that interrogates its type argument -- slick's
//! `ShapedValue.mapToImpl` asks `isCaseClass`, then the companion and whether
//! it has a `tupled`, then every case accessor field's type -- is asking about
//! the symbol nsc's typer has built for the class. scala-rs keeps a different
//! model of the same class, and this module is the translation:
//!
//! * a `val` is **one** symbol here and **two** in nsc: a `private[this]`
//!   field whose name ends in a space, and a getter (plus a setter for a
//!   `var`). A constructor parameter that is not a `val` is a field alone,
//!   without the space; a `lazy val` is its accessor alone. The field and
//!   accessor flags are nsc's (`PARAMACCESSOR`, `CASEACCESSOR`, `STABLE`,
//!   `ACCESSOR`), read off real scalac 2.13.16 for each shape
//!   (`tests/fixtures/gbmac_decls_use.scala`);
//! * a case class's synthetic members are listed the way nsc lists them, in
//!   nsc's order and with nsc's flags and parameter names -- including the
//!   four (`productIterator`, `hashCode`, `toString`, `equals`) scala-rs
//!   leaves to the backend rather than modelling as symbols;
//! * a case-class companion lists `apply` and `unapply` with the class's
//!   parameters, the synthetic companion's `toString`, and the explicit
//!   companion's `writeReplace`.
//!
//! **Nothing is described up front.** The engine receives a class as its
//! identity (`(src <id>)`), and its info -- parents and declarations -- is
//! asked for only when the implementation forces it; each declaration's own
//! type is asked for only when *that* is forced. `mapToImpl` never asks for
//! the type of a `Label`'s `val fontColor = { … }`, so that type is never
//! inferred on its behalf. That is nsc's own order of evaluation, and it is
//! what `docs/macros.md` §5.1 said a snapshot could not give.
//!
//! **What cannot be translated faithfully is refused**, and the refusal
//! reaches the engine as a question scala-rs cannot answer -- a diagnostic
//! naming the class and the reason -- never as a declaration list that is
//! missing something: a nested class or type member, a `val` in a trait, a
//! case class with a second parameter list, a case class whose synthetic
//! members could be suppressed by an ancestor this compiler cannot see into.

use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_pickle::names::encode_method_name;

use crate::check::Typer;
use crate::expand::quote_into;
use crate::symbol::SymKind;

/// The members of `scala.Product` and `scala.Equals` a case class gets from
/// nsc, in the order nsc's `SyntheticMethods` adds them, after `copy`.
const CASE_SYNTHETICS: [&str; 9] = [
    "productPrefix",
    "productArity",
    "productElement",
    "productIterator",
    "canEqual",
    "productElementName",
    "hashCode",
    "toString",
    "equals",
];

/// Ancestors whose members never suppress a case class's synthetic methods:
/// nsc's `hasOverridingImplementation` skips the very symbols the synthetics
/// stand in for, and these classes declare nothing else of those names.
const STANDARD_CASE_ANCESTORS: [&str; 6] = [
    "java/lang/Object",
    "scala/Any",
    "scala/Product",
    "scala/Equals",
    "java/io/Serializable",
    "scala/Serializable",
];

/// One declaration of a described class, as it goes over the wire.
enum Decl {
    /// A scala-rs symbol that is also exactly one nsc symbol -- a method or
    /// a constructor. Its info is asked for by identity, when forced.
    Member {
        id: SymbolId,
        name: String,
        flags: Vec<&'static str>,
    },
    /// One of the symbols nsc makes of a scala-rs `val`: its `getter`,
    /// `setter` or `field`. Its info is asked for as that view of `id`.
    View {
        id: SymbolId,
        name: String,
        view: &'static str,
        flags: Vec<&'static str>,
    },
    /// A member nsc synthesises that scala-rs has no symbol for, whose
    /// signature is fixed by the language: sent with its info.
    Eager {
        name: String,
        flags: Vec<&'static str>,
        info: String,
    },
}

impl Decl {
    fn wire(&self) -> String {
        let flags = |fs: &[&str]| {
            let mut out = String::from("(f");
            for f in fs {
                out.push(' ');
                quote_into(&mut out, f);
            }
            out.push(')');
            out
        };
        let mut out = String::new();
        match self {
            Decl::Member {
                id,
                name,
                flags: fs,
            } => {
                out.push_str(&format!("(dm {} ", id.0));
                quote_into(&mut out, &encode_method_name(name));
                out.push(' ');
                out.push_str(&flags(fs));
                out.push(')');
            }
            Decl::View {
                id,
                name,
                view,
                flags: fs,
            } => {
                out.push_str(&format!("(dv {} ", id.0));
                quote_into(&mut out, name);
                out.push_str(&format!(" {view} "));
                out.push_str(&flags(fs));
                out.push(')');
            }
            Decl::Eager {
                name,
                flags: fs,
                info,
            } => {
                out.push_str("(de ");
                quote_into(&mut out, &encode_method_name(name));
                out.push(' ');
                out.push_str(&flags(fs));
                out.push(' ');
                out.push_str(info);
                out.push(')');
            }
        }
        out
    }
}

/// The access and modifier flags a member keeps on each of the symbols nsc
/// makes of it.
fn modifier_flags(flags: Flags, out: &mut Vec<&'static str>) {
    for (bit, name) in [
        (Flags::PROTECTED, "PROTECTED"),
        (Flags::OVERRIDE, "OVERRIDE"),
        (Flags::FINAL, "FINAL"),
        (Flags::IMPLICIT, "IMPLICIT"),
    ] {
        if flags.contains(bit) {
            out.push(name);
        }
    }
}

fn quoted(s: &str) -> String {
    let mut out = String::new();
    quote_into(&mut out, s);
    out
}

impl Typer {
    /// `(classinfo (parents …) (decls …))` for a class or module class this
    /// run is compiling.
    pub(crate) fn mirror_class_info(&mut self, cls: SymbolId) -> Result<String, String> {
        let name = self.st.get(cls).name.clone();
        if !self.st.get(cls).tparams.is_empty() {
            return Err(format!("`{name}` has type parameters"));
        }
        let parents = self.mirror_parents(cls)?;
        let decls = match self.st.get(cls).kind {
            SymKind::Class => self.mirror_class_decls(cls)?,
            SymKind::ModuleClass => self.mirror_module_decls(cls)?,
            other => return Err(format!("`{name}` is a {other:?}, not a class")),
        };
        let decls: Vec<String> = decls.iter().map(Decl::wire).collect();
        Ok(format!(
            "(classinfo (parents {}) (decls {}))",
            parents.join(" "),
            decls.join(" ")
        ))
    }

    /// The case class a module class is the companion of, if it is one.
    fn case_companion_class(&self, module_cls: SymbolId) -> Option<SymbolId> {
        let owner = self.st.get(module_cls).owner;
        let module = self.st.get(owner).members.iter().copied().find(|&m| {
            self.st.get(m).kind == SymKind::Module && self.st.module_class_of(m) == module_cls
        })?;
        let name = self.st.get(module).name.clone();
        self.st.get(owner).members.iter().copied().find(|&c| {
            self.st.get(c).kind == SymKind::Class
                && self.st.get(c).name == name
                && self.st.get(c).flags.contains(Flags::CASE)
        })
    }

    fn mirror_parents(&mut self, cls: SymbolId) -> Result<Vec<String>, String> {
        let mut parents = self.st.get(cls).parents.clone();
        if self.st.get(cls).kind == SymKind::ModuleClass && self.case_companion_class(cls).is_some()
        {
            let serializable = |st: &crate::symbol::SymbolTable, t: &Type| {
                st.class_sym_of(t)
                    .is_some_and(|s| st.jvm_internal(s) == "java/io/Serializable")
            };
            if self.st.get(cls).flags.contains(Flags::SYNTHETIC) {
                // nsc's synthetic companion extends `AbstractFunctionN` and
                // nothing else while the typer runs; `Serializable` is added
                // to the class file later (real scalac 2.13.16: `baseClasses`
                // is `Foo, AbstractFunction2, Function2, Object, Any`).
                parents.retain(|p| !serializable(&self.st, p));
            } else if !parents.iter().any(|p| serializable(&self.st, p)) {
                // An explicit companion of a case class is made
                // `Serializable` by nsc's namer (`WithObj, Serializable,
                // Object, Any`); scala-rs leaves that to the backend.
                parents.push(Type::Class {
                    sym: crate::classpath::find_by_jvm(&self.st, "java/io/Serializable")
                        .ok_or("java.io.Serializable is not loaded")?,
                    args: Vec::new(),
                });
            }
        }
        let mut out = Vec::new();
        for p in parents {
            // `scala.AnyRef` is `java.lang.Object` -- 2.13 declares it as that
            // alias -- and the engine's mirror has no `staticClass` for the
            // alias, only for the class it names.
            out.push(if matches!(p, Type::AnyRef) {
                "(ty \"java.lang.Object\")".to_string()
            } else {
                self.type_to_wire(&p)?
            });
        }
        if out.is_empty() {
            out.push("(ty \"java.lang.Object\")".to_string());
        }
        Ok(out)
    }

    /// The symbols nsc makes of one `val` or `var`, constructor parameter or
    /// body member.
    fn val_views(
        &self,
        id: SymbolId,
        param: bool,
        case_accessor: bool,
        has_accessor: bool,
        in_trait: bool,
        out: &mut Vec<Decl>,
    ) -> Result<(), String> {
        let s = self.st.get(id);
        let name = s.name.clone();
        let flags = s.flags;
        if let Some(within) = &s.private_within {
            return Err(qualified_access(&name, within));
        }
        let mutable = flags.contains(Flags::MUTABLE);
        let local = flags.contains(Flags::LOCAL);
        let private = flags.contains(Flags::PRIVATE);
        let protected = flags.contains(Flags::PROTECTED);
        // What an accessor carries of the definition's modifiers: its access
        // (`private`, `protected`, `protected[this]`), `final`, `implicit`
        // and `override`. The field keeps only `final`.
        let accessor_mods = |fs: &mut Vec<&'static str>| {
            if private && !local {
                fs.push("PRIVATE");
            }
            if protected {
                fs.push("PROTECTED");
                if local {
                    fs.push("LOCAL");
                }
            }
            for (bit, n) in [
                (Flags::OVERRIDE, "OVERRIDE"),
                (Flags::FINAL, "FINAL"),
                (Flags::IMPLICIT, "IMPLICIT"),
            ] {
                if flags.contains(bit) {
                    fs.push(n);
                }
            }
        };
        if flags.contains(Flags::LAZY) {
            // 2.12+: a `lazy val` is its accessor alone until the fields
            // phase, long after the typer.
            let mut fs = vec!["STABLE", "ACCESSOR", "LAZY"];
            accessor_mods(&mut fs);
            out.push(Decl::View {
                id,
                name,
                view: "getter",
                flags: fs,
            });
            return Ok(());
        }
        let deferred = s.deferred_val;
        // A trait's `val` is its accessors alone while the typer runs (the
        // field is the fields phase's), and so is an abstract `val` anywhere.
        let has_field = !in_trait && !deferred;
        let mut field = vec!["PRIVATE", "LOCAL"];
        if param {
            field.push("PARAMACCESSOR");
        }
        if case_accessor {
            field.push("CASEACCESSOR");
        }
        if mutable {
            field.push("MUTABLE");
        }
        if flags.contains(Flags::FINAL) {
            field.push("FINAL");
        }
        if !has_accessor {
            // `private[this] val x` and a constructor parameter that is not
            // a `val`: the field alone, under the name as written.
            if !has_field {
                return Err(format!(
                    "`{name}` is a `private[this]` value with no field, which scala-rs \
                     cannot describe to the engine"
                ));
            }
            out.push(Decl::View {
                id,
                name,
                view: "field",
                flags: field,
            });
            return Ok(());
        }
        let mut getter = Vec::new();
        if !mutable {
            getter.push("STABLE");
        }
        getter.push("ACCESSOR");
        if deferred {
            getter.push("DEFERRED");
        }
        if param {
            getter.push("PARAMACCESSOR");
        }
        if case_accessor {
            getter.push("CASEACCESSOR");
        }
        accessor_mods(&mut getter);
        out.push(Decl::View {
            id,
            name: name.clone(),
            view: "getter",
            flags: getter,
        });
        if mutable {
            let mut setter = vec!["ACCESSOR"];
            if deferred {
                setter.push("DEFERRED");
            }
            if param {
                setter.push("PARAMACCESSOR");
            }
            if private && !local {
                setter.push("PRIVATE");
            }
            if protected {
                setter.push("PROTECTED");
            }
            out.push(Decl::View {
                id,
                name: encode_method_name(&format!("{name}_=")),
                view: "setter",
                flags: setter,
            });
        }
        if has_field {
            out.push(Decl::View {
                id,
                name: format!("{name} "),
                view: "field",
                flags: field,
            });
        }
        Ok(())
    }

    /// The flags nsc gives a method scala-rs models as one.
    fn method_flags(&self, id: SymbolId) -> Vec<&'static str> {
        let flags = self.st.get(id).flags;
        let mut fs = Vec::new();
        if flags.contains(Flags::ABSTRACT) {
            fs.push("DEFERRED");
        }
        if flags.contains(Flags::PRIVATE) {
            fs.push("PRIVATE");
            if flags.contains(Flags::LOCAL) {
                fs.push("LOCAL");
            }
        }
        if flags.contains(Flags::SYNTHETIC) {
            fs.push("SYNTHETIC");
        }
        if self.st.get(id).name.contains("$default$") {
            fs.push("DEFAULTPARAM");
        }
        modifier_flags(flags, &mut fs);
        fs
    }

    fn mirror_class_decls(&mut self, cls: SymbolId) -> Result<Vec<Decl>, String> {
        let s = self.st.get(cls).clone();
        let name = s.name.clone();
        let is_case = s.flags.contains(Flags::CASE);
        let is_trait = s.flags.contains(Flags::TRAIT);
        let ctor =
            s.members.iter().copied().find(|&m| {
                self.st.get(m).kind == SymKind::Method && self.st.get(m).name == "<init>"
            });
        if is_case {
            if let Some(c) = ctor {
                if self.st.get(c).paramss.len() > 1 {
                    return Err(format!(
                        "`{name}` is a case class with more than one parameter list"
                    ));
                }
            }
        }
        let mut out = Vec::new();
        if is_trait && !self.is_pure_interface(cls) {
            // A trait's initialiser, which nsc's typer declares for every
            // trait that has anything to initialise or any concrete member.
            out.push(Decl::Eager {
                name: "$init$".to_string(),
                flags: Vec::new(),
                info: "(method (params) (ty \"scala.Unit\"))".to_string(),
            });
        }
        for &f in &s.ctor_fields {
            if self.st.get(f).kind != SymKind::Term {
                return Err(format!(
                    "`{}`, a constructor parameter of `{name}`, is not a value",
                    self.st.get(f).name
                ));
            }
            let flags = self.st.get(f).flags;
            let has_accessor =
                (is_case && !flags.contains(Flags::LOCAL)) || flags.contains(Flags::ACCESSOR);
            self.val_views(f, true, is_case, has_accessor, false, &mut out)?;
        }
        // A trait has no constructor in nsc, only the `$init$` above.
        if let Some(c) = ctor.filter(|_| !is_trait) {
            out.push(Decl::Member {
                id: c,
                name: "<init>".to_string(),
                flags: Vec::new(),
            });
        }
        let synthetic = |st: &crate::symbol::SymbolTable, m: SymbolId| {
            st.get(m).flags.contains(Flags::SYNTHETIC)
        };
        let mut copies = Vec::new();
        let mut copy_defaults = Vec::new();
        let mut modelled: Vec<(String, SymbolId)> = Vec::new();
        for m in s.members.clone() {
            if Some(m) == ctor || s.ctor_fields.contains(&m) {
                continue;
            }
            let ms = self.st.get(m).clone();
            if is_case && synthetic(&self.st, m) {
                if ms.name == "copy" {
                    copies.push(m);
                    continue;
                }
                if ms.name.starts_with("copy$default$") {
                    copy_defaults.push(m);
                    continue;
                }
                if CASE_SYNTHETICS.contains(&ms.name.as_str()) {
                    modelled.push((ms.name.clone(), m));
                    continue;
                }
            }
            match ms.kind {
                SymKind::Term => {
                    let local_only =
                        ms.flags.contains(Flags::PRIVATE) && ms.flags.contains(Flags::LOCAL);
                    self.val_views(m, false, false, !local_only, is_trait, &mut out)?;
                }
                SymKind::Method => {
                    if let Some(within) = &ms.private_within {
                        return Err(qualified_access(&ms.name, within));
                    }
                    let flags = self.method_flags(m);
                    out.push(Decl::Member {
                        id: m,
                        name: ms.name.clone(),
                        flags,
                    });
                }
                other => {
                    return Err(format!(
                        "`{}` is {}, which scala-rs cannot describe to the engine",
                        ms.name,
                        member_kind(other)
                    ))
                }
            }
        }
        if !is_case {
            return Ok(out);
        }
        for c in copies {
            out.push(Decl::Member {
                id: c,
                name: "copy".to_string(),
                flags: vec!["SYNTHETIC"],
            });
        }
        copy_defaults.sort_by_key(|&m| self.st.get(m).name.clone());
        for d in copy_defaults {
            out.push(Decl::Member {
                id: d,
                name: self.st.get(d).name.clone(),
                flags: vec!["SYNTHETIC", "DEFAULTPARAM"],
            });
        }
        for name in CASE_SYNTHETICS {
            let has_symbol = modelled.iter().any(|(n, _)| n == name);
            if !has_symbol && !self.nsc_synthesizes(cls, name)? {
                continue;
            }
            let (flags, info) = synthetic_signature(name);
            out.push(Decl::Eager {
                name: name.to_string(),
                flags,
                info,
            });
        }
        Ok(out)
    }

    /// A trait every member of which is abstract: nsc's `INTERFACE`, which
    /// has no `$init$` (`isPureInterfaceMember`).
    pub(crate) fn is_pure_interface(&self, cls: SymbolId) -> bool {
        let s = self.st.get(cls);
        s.flags.contains(Flags::TRAIT)
            && s.members.iter().all(|&m| {
                let ms = self.st.get(m);
                match ms.kind {
                    SymKind::Method => ms.name == "<init>" || ms.flags.contains(Flags::ABSTRACT),
                    SymKind::Term => ms.deferred_val,
                    SymKind::TypeMember => true,
                    _ => false,
                }
            })
    }

    /// Whether nsc adds the synthetic `name` to case class `cls` -- for a
    /// member scala-rs leaves to its backend rather than modelling.
    ///
    /// nsc's `SyntheticMethods.hasOverridingImplementation`: not when the
    /// class declares one itself, nor when it inherits a concrete one from
    /// anywhere but the classes the synthetics stand in for. An ancestor read
    /// from a class file may declare one this compiler has not loaded, so
    /// such an ancestor is a refusal rather than a guess.
    fn nsc_synthesizes(&mut self, cls: SymbolId, name: &str) -> Result<bool, String> {
        if self
            .st
            .get(cls)
            .members
            .iter()
            .any(|&m| self.st.get(m).name == name)
        {
            return Ok(false);
        }
        let mut work: Vec<Type> = self.st.get(cls).parents.clone();
        let mut seen = std::collections::HashSet::new();
        while let Some(p) = work.pop() {
            if matches!(p, Type::AnyRef | Type::Any) {
                continue;
            }
            let Some(pid) = self.st.class_sym_of(&p) else {
                return Err(format!(
                    "a parent of `{}` is `{}`, which is not a class",
                    self.st.get(cls).name,
                    self.st.display_type(&p)
                ));
            };
            if !seen.insert(pid.0) {
                continue;
            }
            let jvm = self.st.jvm_internal(pid);
            if STANDARD_CASE_ANCESTORS.contains(&jvm.as_str()) {
                continue;
            }
            if !self.is_current_run_class(pid) {
                return Err(format!(
                    "`{}` inherits from `{}`, a class read from the classpath, which may \
                     declare a `{name}` that keeps nsc from synthesising one",
                    self.st.get(cls).name,
                    jvm.replace('/', ".")
                ));
            }
            let concrete = self.st.get(pid).members.iter().any(|&m| {
                let ms = self.st.get(m);
                ms.name == name
                    && matches!(ms.kind, SymKind::Method | SymKind::Term)
                    && !ms.flags.contains(Flags::ABSTRACT)
                    && !ms.deferred_val
                    && !ms.flags.contains(Flags::SYNTHETIC)
            });
            if concrete {
                return Ok(false);
            }
            work.extend(self.st.get(pid).parents.clone());
        }
        Ok(true)
    }

    fn mirror_module_decls(&mut self, module_cls: SymbolId) -> Result<Vec<Decl>, String> {
        let s = self.st.get(module_cls).clone();
        let case_class = self.case_companion_class(module_cls);
        let synthetic_companion = s.flags.contains(Flags::SYNTHETIC) && case_class.is_some();
        let mut out = vec![Decl::Eager {
            name: "<init>".to_string(),
            flags: Vec::new(),
            info: "(method (params) (selftype))".to_string(),
        }];
        if synthetic_companion {
            let (_, info) = synthetic_signature("toString");
            out.push(Decl::Eager {
                name: "toString".to_string(),
                flags: vec!["FINAL", "OVERRIDE", "SYNTHETIC"],
                info,
            });
        }
        let mut apply = None;
        let mut unapply = None;
        let mut apply_defaults = Vec::new();
        let mut ctor_defaults = Vec::new();
        for m in s.members.clone() {
            let ms = self.st.get(m).clone();
            if case_class.is_some() && ms.flags.contains(Flags::SYNTHETIC) {
                match ms.name.as_str() {
                    "apply" if ms.flags.contains(Flags::CASE) => {
                        apply = Some(m);
                        continue;
                    }
                    "unapply" if ms.flags.contains(Flags::CASE) => {
                        unapply = Some(m);
                        continue;
                    }
                    n if n.starts_with("apply$default$") => {
                        apply_defaults.push(m);
                        continue;
                    }
                    n if n.starts_with("$lessinit$greater$default$")
                        || n.starts_with("<init>$default$") =>
                    {
                        ctor_defaults.push(m);
                        continue;
                    }
                    _ => {}
                }
            }
            match ms.kind {
                SymKind::Method if ms.name == "<init>" => {}
                SymKind::Method => {
                    if let Some(within) = &ms.private_within {
                        return Err(qualified_access(&ms.name, within));
                    }
                    let flags = self.method_flags(m);
                    out.push(Decl::Member {
                        id: m,
                        name: ms.name.clone(),
                        flags,
                    });
                }
                SymKind::Term => {
                    let local_only =
                        ms.flags.contains(Flags::PRIVATE) && ms.flags.contains(Flags::LOCAL);
                    self.val_views(m, false, false, !local_only, false, &mut out)?;
                }
                other => {
                    return Err(format!(
                        "`{}` is {}, which scala-rs cannot describe to the engine",
                        ms.name,
                        member_kind(other)
                    ))
                }
            }
        }
        let Some(class) = case_class else {
            return Ok(out);
        };
        if let Some(a) = apply {
            let info = self.companion_apply_info(class, a)?;
            out.push(Decl::Eager {
                name: "apply".to_string(),
                flags: vec!["CASE", "SYNTHETIC"],
                info,
            });
        }
        apply_defaults.sort_by_key(|&m| self.st.get(m).name.clone());
        for d in apply_defaults {
            out.push(Decl::Member {
                id: d,
                name: self.st.get(d).name.clone(),
                flags: vec!["SYNTHETIC", "DEFAULTPARAM"],
            });
        }
        if let Some(u) = unapply {
            let class_ty = self.type_to_wire(&Type::Class {
                sym: class,
                args: Vec::new(),
            })?;
            let ret = match self.st.get(u).ty.clone() {
                Type::Method { ret, .. } if !ret.is_no_type() => self.type_to_wire(&ret)?,
                _ => {
                    return Err(format!(
                        "the `unapply` of `{}` has no type yet",
                        self.st.get(class).name
                    ))
                }
            };
            out.push(Decl::Eager {
                name: "unapply".to_string(),
                flags: vec!["CASE", "SYNTHETIC"],
                info: format!("(method (params (argn \"x$0\" (f \"PARAM\") {class_ty})) {ret})"),
            });
        }
        ctor_defaults.sort_by_key(|&m| self.st.get(m).name.clone());
        for d in ctor_defaults {
            out.push(Decl::Member {
                id: d,
                name: self.st.get(d).name.clone(),
                flags: vec!["SYNTHETIC", "DEFAULTPARAM"],
            });
        }
        if !synthetic_companion {
            out.push(Decl::Eager {
                name: "writeReplace".to_string(),
                flags: vec!["PRIVATE", "SYNTHETIC"],
                info: "(method (params) (ty \"java.lang.Object\"))".to_string(),
            });
        }
        Ok(out)
    }

    /// A case companion's `apply`: the class's own constructor parameters,
    /// defaults and all, returning the class.
    fn companion_apply_info(&mut self, class: SymbolId, apply: SymbolId) -> Result<String, String> {
        let ptys = match self.st.get(apply).ty.clone() {
            Type::Method { paramss, .. } if paramss.len() == 1 => paramss[0].clone(),
            _ => {
                return Err(format!(
                    "the `apply` of `{}` has no single parameter list",
                    self.st.get(class).name
                ))
            }
        };
        let fields = self.st.get(class).ctor_fields.clone();
        if fields.len() != ptys.len() {
            return Err(format!(
                "the `apply` of `{}` does not match its constructor",
                self.st.get(class).name
            ));
        }
        let mut params = Vec::new();
        for (f, t) in fields.iter().zip(&ptys) {
            let t = if t.is_no_type() {
                self.st.get(*f).ty.clone()
            } else {
                t.clone()
            };
            if t.is_no_type() {
                return Err(format!(
                    "the parameter `{}` of `{}` has no type yet",
                    self.st.get(*f).name,
                    self.st.get(class).name
                ));
            }
            let fl = if self.st.get(*f).flags.contains(Flags::DEFAULTPARAM) {
                "(f \"PARAM\" \"DEFAULTPARAM\")"
            } else {
                "(f \"PARAM\")"
            };
            params.push(format!(
                "(argn {} {fl} {})",
                quoted(&self.st.get(*f).name),
                self.type_to_wire(&t)?
            ));
        }
        let class_ty = self.type_to_wire(&Type::Class {
            sym: class,
            args: Vec::new(),
        })?;
        Ok(format!("(method (params {}) {class_ty})", params.join(" ")))
    }

    /// The info of one view of a `val`: `getter`, `setter` or `field`.
    pub(crate) fn mirror_view_info(&mut self, view: &str, id: SymbolId) -> Result<String, String> {
        let ty = self.settled_type(id)?;
        let ty = self.type_to_wire(&ty)?;
        match view {
            "getter" => Ok(format!("(nullary {ty})")),
            "setter" => Ok(format!(
                "(method (params (argn \"x$1\" (f \"PARAM\") {ty})) (ty \"scala.Unit\"))"
            )),
            "field" => Ok(ty),
            other => Err(format!("the macro engine asked for a `{other}` view")),
        }
    }

    /// The type of a value, inferring it if nothing has yet -- the way nsc's
    /// `typeSignature` forces a lazy type. A value whose inference is already
    /// under way is the cycle nsc reports as "recursive value needs type";
    /// here it is a refusal, since the question came from the macro and not
    /// from the program.
    fn settled_type(&mut self, id: SymbolId) -> Result<Type, String> {
        let unsettled = |ty: &Type| match ty {
            Type::Method { ret, .. } => ret.is_no_type(),
            t => t.is_no_type(),
        };
        if unsettled(&self.st.get(id).ty) && !self.lazy_completing.contains(&id) {
            let span = self.macro_rpc_span;
            self.complete_lazy_sig(id, span);
        }
        let ty = self.st.get(id).ty.clone();
        if unsettled(&ty) {
            return Err(format!(
                "recursive value {} needs type",
                self.st.get(id).name
            ));
        }
        Ok(match ty {
            Type::Method { paramss, ret } if paramss.is_empty() => *ret,
            t => t,
        })
    }

    /// The companion of a class, object or module class, for the engine to
    /// enter beside it: nsc finds a companion by looking its name up in the
    /// owner's declarations, so both have to be there.
    pub(crate) fn mirror_companion(&self, id: SymbolId) -> SymbolId {
        let s = self.st.get(id);
        let owner = s.owner;
        match s.kind {
            SymKind::Class => self
                .st
                .companion_module(id)
                .filter(|&m| m != id)
                .unwrap_or(SymbolId::NONE),
            SymKind::Module | SymKind::ModuleClass => {
                let module = self.mirror_module_pair(id).map(|(m, _)| m);
                let Some(module) = module else {
                    return SymbolId::NONE;
                };
                let name = self.st.get(module).name.clone();
                self.st
                    .get(owner)
                    .members
                    .iter()
                    .copied()
                    .find(|&c| self.st.get(c).kind == SymKind::Class && self.st.get(c).name == name)
                    .unwrap_or(SymbolId::NONE)
            }
            _ => SymbolId::NONE,
        }
    }

    /// An object's module symbol and module class, from either.
    pub(crate) fn mirror_module_pair(&self, id: SymbolId) -> Option<(SymbolId, SymbolId)> {
        let s = self.st.get(id);
        match s.kind {
            SymKind::Module => Some((id, self.st.module_class_of(id))),
            SymKind::ModuleClass => {
                let owner = s.owner;
                self.st
                    .get(owner)
                    .members
                    .iter()
                    .copied()
                    .find(|&m| {
                        self.st.get(m).kind == SymKind::Module && self.st.module_class_of(m) == id
                    })
                    .map(|m| (m, id))
            }
            _ => None,
        }
    }
}

/// A qualified access boundary (`private[p]`, `protected[p]`) is a symbol
/// reference on nsc's side, which the mirror does not carry; described
/// without it, the member would read as plainly `private`.
fn qualified_access(name: &str, within: &str) -> String {
    format!(
        "`{name}` is accessible within `{within}` only, a qualifier the mirror \
         does not carry to the engine"
    )
}

/// What a member scala-rs cannot describe is, for the refusal.
fn member_kind(kind: SymKind) -> &'static str {
    match kind {
        SymKind::Class => "a nested class",
        SymKind::Module | SymKind::ModuleClass => "a nested object",
        SymKind::TypeMember => "a type member",
        SymKind::TypeParam => "a type parameter",
        _ => "a member of a kind",
    }
}

/// The flags and signature nsc gives each synthetic case-class member, as
/// real scalac 2.13.16 reports them while its typer runs.
fn synthetic_signature(name: &str) -> (Vec<&'static str>, String) {
    let int = "(ty \"scala.Int\")";
    let any = "(ty \"scala.Any\")";
    let string = "(ty \"java.lang.String\")";
    let boolean = "(ty \"scala.Boolean\")";
    let x1 = |t: &str| format!("(params (argn \"x$1\" (f \"PARAM\") {t}))");
    match name {
        "productPrefix" => (vec!["OVERRIDE", "SYNTHETIC"], format!("(nullary {string})")),
        "productArity" => (vec!["SYNTHETIC"], format!("(nullary {int})")),
        "productElement" => (vec!["SYNTHETIC"], format!("(method {} {any})", x1(int))),
        "productIterator" => (
            vec!["OVERRIDE", "SYNTHETIC"],
            "(nullary (ty \"scala.collection.Iterator\" (ty \"scala.Any\")))".to_string(),
        ),
        "canEqual" => (vec!["SYNTHETIC"], format!("(method {} {boolean})", x1(any))),
        "productElementName" => (
            vec!["OVERRIDE", "SYNTHETIC"],
            format!("(method {} {string})", x1(int)),
        ),
        "hashCode" => (
            vec!["OVERRIDE", "SYNTHETIC"],
            format!("(method (params) {int})"),
        ),
        "toString" => (
            vec!["OVERRIDE", "SYNTHETIC"],
            format!("(method (params) {string})"),
        ),
        "equals" => (
            vec!["OVERRIDE", "SYNTHETIC"],
            format!("(method {} {boolean})", x1(any)),
        ),
        _ => (Vec::new(), "(notype)".to_string()),
    }
}
