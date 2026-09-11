//! Deprecation warnings: nsc's `RefChecks.checkUndesiredProperties` for a
//! reference to a `@deprecated` symbol (a term, or a class named in a type),
//! and `UnCurry.arrayToSequence`'s "Passing an explicit array value to a
//! Scala varargs method is deprecated".
//!
//! A library symbol's `@deprecated` comes from its pickle (the prelude's
//! hand-written copies of library members carry none); a source symbol's
//! from its own annotations. By default nsc does not print these but counts
//! them per `since` version (`scala_rs_span::finish_diagnostics`), so the
//! count has to be right as much as the text.

use crate::check::Typer;
use crate::symbol::SymKind;
use crate::warn_util::{point_of, warning_at};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Phase, WarnCategory};
use std::collections::HashMap;

pub(crate) fn run(t: &mut Typer, units: &mut [(&mut Tree, usize)]) {
    let mut cache: HashMap<SymbolId, Option<(String, String)>> = HashMap::new();
    let mut out = Vec::new();
    for (tree, file) in units.iter() {
        let Some(src) = t.sources.get(*file).cloned() else {
            continue;
        };
        let mut pass = Depr {
            t: &mut *t,
            src: &src,
            file: *file,
            out: Vec::new(),
            cache: &mut cache,
            in_deprecated: 0,
        };
        pass.visit(tree);
        out.extend(pass.out);
    }
    t.diags.extend(out);
}

struct Depr<'a> {
    t: &'a mut Typer,
    src: &'a str,
    file: usize,
    out: Vec<Diagnostic>,
    /// `@deprecated` symbol -> (the warning's text, `since`).
    cache: &'a mut HashMap<SymbolId, Option<(String, String)>>,
    /// Inside a deprecated definition: nsc does not warn there.
    in_deprecated: usize,
}

const VARARGS_ARRAY: &str = "Passing an explicit array value to a Scala varargs method is deprecated (since 2.13.0) and will result in a defensive copy; Use the more efficient non-copying ArraySeq.unsafeWrapArray or an explicit toIndexedSeq call";

impl<'a> Depr<'a> {
    fn push(&mut self, point: u32, msg: String, since: String, phase: Phase) {
        let fatal = self.t.fatal_warnings;
        let d = warning_at(self.file, point, point + 1, msg, phase, fatal)
            .with_category(WarnCategory::Deprecation { since });
        self.out.push(d);
    }

    fn check_sym(&mut self, sym: SymbolId, point: u32) {
        if self.in_deprecated > 0 || sym.is_none() {
            return;
        }
        if let Some((msg, since)) = self.deprecation_of(sym) {
            self.push(point, msg, since, Phase::Refchecks);
        }
    }

    fn source_deprecated(&self, annots: &[Tree]) -> Option<(String, String)> {
        let a = annots.iter().find(|a| {
            let p = a.annotation_path();
            p == "deprecated" || p == "scala.deprecated"
        })?;
        let mut message = String::new();
        let mut since = String::new();
        let mut args: &[Tree] = &[];
        let mut cur = a;
        while let TreeKind::Apply { fun, args: xs } = &cur.kind {
            args = xs;
            cur = fun;
        }
        for (i, arg) in args.iter().enumerate() {
            let (slot, value) = match &arg.kind {
                TreeKind::Assign { lhs, rhs } => (lhs.name().unwrap_or("").to_string(), &**rhs),
                _ => ((if i == 0 { "message" } else { "since" }).to_string(), arg),
            };
            if let TreeKind::Literal { lit: Lit::String(s) } = &value.kind {
                if slot == "message" {
                    message = s.clone();
                } else if slot == "since" {
                    since = s.clone();
                }
            }
        }
        Some((message, since))
    }

    /// `Some((text, since))` when `sym` is `@deprecated`.
    fn deprecation_of(&mut self, sym: SymbolId) -> Option<(String, String)> {
        if let Some(c) = self.cache.get(&sym) {
            return c.clone();
        }
        let r = self.compute(sym);
        self.cache.insert(sym, r.clone());
        r
    }

    fn compute(&mut self, sym: SymbolId) -> Option<(String, String)> {
        let s = self.t.st.get(sym);
        if matches!(s.kind, SymKind::Package | SymKind::TypeParam | SymKind::NoSymbol) {
            return None;
        }
        if s.flags.contains(Flags::PARAM) {
            return None;
        }
        let (message, since, desc) = if let Some((m, v)) = self.source_deprecated(&s.annotations.clone()) {
            (m, v, self.describe_source(sym))
        } else {
            let (m, v) = self.library_deprecation(sym)?;
            (m, v, self.describe_library(sym))
        };
        let since_txt = if since.is_empty() {
            String::new()
        } else {
            format!(" (since {since})")
        };
        let msg_txt = if message.is_empty() {
            String::new()
        } else {
            format!(": {message}")
        };
        Some((format!("{desc} is deprecated{since_txt}{msg_txt}"), since))
    }

    /// The pickled `@deprecated` of a library symbol.
    fn library_deprecation(&mut self, sym: SymbolId) -> Option<(String, String)> {
        let s = self.t.st.get(sym);
        if !self.t.library_abi {
            return None;
        }
        let class_like = matches!(s.kind, SymKind::Class | SymKind::ModuleClass | SymKind::Module | SymKind::TypeMember);
        if class_like {
            let cls = if s.kind == SymKind::Module {
                self.t.st.module_class_of(sym)
            } else {
                sym
            };
            if self.t.st.get(cls).jvm_name.starts_with("scala/") && self.t.st.get(cls).kind != SymKind::TypeMember {
                let sig = self.t.pickle.class_sig_of(&self.t.st, &mut self.t.binary, cls)?;
                return sig.deprecated.as_ref().map(|d| (d.message.clone(), d.since.clone()));
            }
            if s.kind == SymKind::TypeMember || s.kind == SymKind::Module {
                return self.member_deprecation(sym);
            }
            return None;
        }
        self.member_deprecation(sym)
    }

    fn member_deprecation(&mut self, sym: SymbolId) -> Option<(String, String)> {
        let s = self.t.st.get(sym);
        let owner = s.owner;
        if owner.is_none() {
            return None;
        }
        // The prelude's value classes carry the JVM name of their box
        // (`java/lang/Integer`); their members are pickled in `scala.Int`.
        let st = &self.t.st;
        let primitive = [
            (st.int_sym, "scala.Int"),
            (st.long_sym, "scala.Long"),
            (st.short_sym, "scala.Short"),
            (st.byte_sym, "scala.Byte"),
            (st.char_sym, "scala.Char"),
            (st.float_sym, "scala.Float"),
            (st.double_sym, "scala.Double"),
            (st.boolean_sym, "scala.Boolean"),
            (st.unit_sym, "scala.Unit"),
        ]
        .iter()
        .find(|(p, _)| *p == owner)
        .map(|(_, n)| *n);
        if primitive.is_none() && !st.get(owner).jvm_name.starts_with("scala/") {
            return None;
        }
        let name = s.name.clone();
        let our_params = param_names(&self.t.st, &s.ty);
        let sig = match primitive {
            Some(full) => self.t.pickle.class_sig_by_name(&mut self.t.binary, full, false),
            None => self.t.pickle.class_sig_of(&self.t.st, &mut self.t.binary, owner),
        };
        let sig = sig?;
        let cands: Vec<&scala_rs_pickle::sym::Member> = sig.members_named(&name).collect();
        if cands.is_empty() {
            return None;
        }
        let pick: Vec<&scala_rs_pickle::sym::Member> = if cands.len() == 1 {
            cands
        } else {
            let by_shape: Vec<_> = cands
                .iter()
                .copied()
                .filter(|m| sig_param_names(&m.ty).as_ref() == our_params.as_ref())
                .collect();
            if by_shape.is_empty() {
                cands
            } else {
                by_shape
            }
        };
        let first = pick[0].deprecated.clone();
        if pick.iter().all(|m| m.deprecated == first) {
            first.map(|d| (d.message, d.since))
        } else {
            None
        }
    }

    /// nsc's `s"$sym${sym.locationString}"` for a source symbol.
    fn describe_source(&self, sym: SymbolId) -> String {
        let st = &self.t.st;
        let s = st.get(sym);
        format!("{} {}{}", kind_string(st, sym), s.name.trim_end_matches('$'), self.location(s.owner))
    }

    fn describe_library(&self, sym: SymbolId) -> String {
        self.describe_source(sym)
    }

    fn location(&self, owner: SymbolId) -> String {
        if owner.is_none() {
            return String::new();
        }
        let st = &self.t.st;
        let o = st.get(owner);
        let name = o.name.trim_end_matches('$');
        match o.kind {
            SymKind::Package => format!(" in package {name}"),
            SymKind::ModuleClass | SymKind::Module if name == "package" => {
                // A package object prints as its package.
                let pkg = o.jvm_name.trim_end_matches("/package$").trim_end_matches("/package");
                let last = pkg.rsplit('/').next().unwrap_or(pkg);
                format!(" in package {last}")
            }
            SymKind::ModuleClass | SymKind::Module => format!(" in object {name}"),
            SymKind::Class => format!(" in {} {name}", kind_string(st, owner)),
            SymKind::Method => format!(" in method {name}"),
            _ => String::new(),
        }
    }

    // --- traversal --------------------------------------------------------

    fn is_deprecated_def(&self, sym: SymbolId, mods: &Modifiers) -> bool {
        mods.annotations.iter().any(|a| {
            let p = a.annotation_path();
            p == "deprecated" || p == "scala.deprecated"
        }) || (!sym.is_none() && self.source_deprecated(&self.t.st.get(sym).annotations).is_some())
    }

    fn visit(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::ClassDef { mods, impl_, vparamss, tparams, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                let dep = self.is_deprecated_def(tree.sym, mods);
                self.in_deprecated += usize::from(dep);
                for p in tparams.iter().chain(vparamss.iter().flatten()) {
                    self.visit(p);
                }
                for p in &impl_.parents {
                    self.visit_type(p);
                    self.visit_parent_args(p);
                }
                for s in &impl_.body {
                    self.visit(s);
                }
                self.in_deprecated -= usize::from(dep);
            }
            TreeKind::ModuleDef { mods, impl_, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                let dep = self.is_deprecated_def(tree.sym, mods);
                self.in_deprecated += usize::from(dep);
                for p in &impl_.parents {
                    self.visit_type(p);
                    self.visit_parent_args(p);
                }
                for s in &impl_.body {
                    self.visit(s);
                }
                self.in_deprecated -= usize::from(dep);
            }
            TreeKind::DefDef { mods, tparams, vparamss, tpt, rhs, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                let dep = self.is_deprecated_def(tree.sym, mods);
                self.in_deprecated += usize::from(dep);
                for p in tparams.iter().chain(vparamss.iter().flatten()) {
                    self.visit(p);
                }
                self.visit_type(tpt);
                self.visit(rhs);
                self.in_deprecated -= usize::from(dep);
            }
            TreeKind::ValDef { mods, tpt, rhs, .. } => {
                let dep = self.is_deprecated_def(tree.sym, mods);
                self.in_deprecated += usize::from(dep);
                self.visit_type(tpt);
                self.visit(rhs);
                self.in_deprecated -= usize::from(dep);
            }
            TreeKind::TypeDef { rhs, lo, hi, .. } => {
                self.visit_type(rhs);
                if let Some(l) = lo {
                    self.visit_type(l);
                }
                if let Some(h) = hi {
                    self.visit_type(h);
                }
            }
            TreeKind::Ident { .. } => {
                if !tree.span.is_dummy() {
                    let p = point_of(tree, self.src);
                    self.check_sym(tree.sym, p);
                }
            }
            TreeKind::Select { qual, .. } => {
                self.visit(qual);
                if !tree.span.is_dummy() {
                    let p = point_of(tree, self.src);
                    self.check_sym(tree.sym, p);
                }
            }
            TreeKind::Apply { fun, args } => {
                self.check_varargs_array(fun, args);
                // `1 << 2L`: the typer's constant folder reports the
                // deprecated operation it folds, before refchecks.
                if let TreeKind::Select { qual, .. } = &fun.kind {
                    let lit = |t: &Tree| matches!(t.kind, TreeKind::Literal { .. });
                    if lit(qual) && !args.is_empty() && args.iter().all(lit) && self.in_deprecated == 0 {
                        if let Some((msg, since)) = self.deprecation_of(fun.sym) {
                            let p = point_of(fun, self.src);
                            self.push(p, msg, since, Phase::Typer);
                        }
                        for a in args {
                            self.visit(a);
                        }
                        return;
                    }
                }
                self.visit(fun);
                for a in args {
                    self.visit(a);
                }
            }
            TreeKind::TypeApply { fun, args } => {
                self.visit(fun);
                for a in args {
                    self.visit_type(a);
                }
            }
            TreeKind::Typed { expr, tpt } => {
                self.visit(expr);
                self.visit_type(tpt);
            }
            TreeKind::New { tpt } => match &tpt.kind {
                // `new T { ... }`: the anonymous class's parents are written
                // types, its body ordinary code.
                TreeKind::ClassDef { impl_, .. } => {
                    for p in &impl_.parents {
                        self.visit_type(p);
                        self.visit_parent_args(p);
                    }
                    for s in &impl_.body {
                        self.visit(s);
                    }
                }
                _ => self.visit_type(tpt),
            },
            TreeKind::Match { selector, cases } => {
                self.visit(selector);
                for c in cases {
                    self.visit_pattern(&c.pat);
                    self.visit(&c.guard);
                    self.visit(&c.body);
                }
            }
            TreeKind::Try { block, catches, finalizer } => {
                self.visit(block);
                for c in catches {
                    self.visit_pattern(&c.pat);
                    self.visit(&c.guard);
                    self.visit(&c.body);
                }
                self.visit(finalizer);
            }
            TreeKind::Import { .. } => {}
            _ => for_each_child(tree, &mut |c| self.visit(c)),
        }
    }

    fn visit_parent_args(&mut self, p: &Tree) {
        if let TreeKind::Apply { args, fun } = &p.kind {
            for a in args {
                self.visit(a);
            }
            self.visit_parent_args(fun);
        }
    }

    fn visit_pattern(&mut self, p: &Tree) {
        match &p.kind {
            TreeKind::Typed { expr, tpt } => {
                self.visit_pattern(expr);
                self.visit_type(tpt);
            }
            TreeKind::Bind { body, .. } => self.visit_pattern(body),
            TreeKind::Alternative { trees } => trees.iter().for_each(|t| self.visit_pattern(t)),
            TreeKind::Apply { args, .. } | TreeKind::UnApply { args, .. } => {
                args.iter().for_each(|t| self.visit_pattern(t))
            }
            TreeKind::Select { .. } | TreeKind::Ident { .. } if p.stable_pat => self.visit(p),
            _ => {}
        }
    }

    /// A written type: the classes it names.
    fn visit_type(&mut self, t: &Tree) {
        if t.span.is_dummy() {
            return;
        }
        match &t.kind {
            TreeKind::Ident { .. } | TreeKind::Select { .. } => {
                let p = point_of(t, self.src);
                self.check_sym(t.sym, p);
                if let TreeKind::Select { qual, .. } = &t.kind {
                    if !matches!(qual.kind, TreeKind::Ident { .. } | TreeKind::Select { .. }) {
                        self.visit_type(qual);
                    }
                }
            }
            TreeKind::AppliedTypeTree { tpt, args } => {
                self.visit_type(tpt);
                for a in args {
                    self.visit_type(a);
                }
            }
            TreeKind::Apply { fun, .. } => self.visit_type(fun),
            TreeKind::TypeApply { fun, args } => {
                self.visit_type(fun);
                for a in args {
                    self.visit_type(a);
                }
            }
            TreeKind::CompoundTypeTree { parents, .. } => parents.iter().for_each(|p| self.visit_type(p)),
            TreeKind::AnnotatedTypeTree { tpt, .. } => self.visit_type(tpt),
            TreeKind::ExistentialTypeTree { tpt, .. } => self.visit_type(tpt),
            _ => {}
        }
    }

    /// `UnCurry.transformArgs`: `f(xs: _*)` with an array for a Scala
    /// varargs parameter copies it, and that is deprecated.
    fn check_varargs_array(&mut self, fun: &Tree, args: &[Tree]) {
        if self.in_deprecated > 0 {
            return;
        }
        // Our typer appends an implicit clause's arguments to the explicit
        // ones, so the `xs: _*` is not necessarily last.
        let Some((index, last)) = args.iter().enumerate().find(|(_, a)| {
            matches!(&a.kind, TreeKind::Typed { tpt, .. } if is_repeated_marker(tpt))
        }) else {
            return;
        };
        let TreeKind::Typed { expr, .. } = &last.kind else {
            return;
        };
        let array = match &expr.ty {
            Type::Array(_) => true,
            Type::Class { sym, .. } => self.t.st.is_array_class(*sym) || *sym == self.t.st.array_sym,
            _ => false,
        };
        if !array {
            return;
        }
        let (fsym, clause) = method_and_clause(fun);
        if fsym.is_none() {
            return;
        }
        let s = self.t.st.get(fsym);
        if s.flags.contains(Flags::JAVA)
            || (!s.owner.is_none() && self.t.st.get(s.owner).flags.contains(Flags::JAVA))
            || s.jvm_name.contains("java/")
        {
            return;
        }
        let repeated = match &s.ty {
            Type::Method { paramss, .. } => {
                let direct = paramss.get(clause).and_then(|c| c.get(index));
                let flat = paramss.iter().skip(clause).flatten().nth(index);
                direct.or(flat).is_some_and(|p| matches!(p, Type::Repeated(_)))
            }
            _ => false,
        };
        if repeated {
            let p = point_of(expr, self.src);
            self.push(p, VARARGS_ARRAY.to_string(), "2.13.0".into(), Phase::Uncurry);
        }
    }
}

fn is_repeated_marker(tpt: &Tree) -> bool {
    match &tpt.kind {
        TreeKind::AppliedTypeTree { tpt, .. } => {
            matches!(&tpt.kind, TreeKind::Ident { name } if name == "<repeated>")
        }
        TreeKind::Ident { name } => name == "<repeated>",
        _ => false,
    }
}

/// The method an application calls, and which of its parameter clauses the
/// outermost argument list fills.
fn method_and_clause(fun: &Tree) -> (SymbolId, usize) {
    match &fun.kind {
        TreeKind::Apply { fun, .. } => {
            let (s, c) = method_and_clause(fun);
            (s, c + 1)
        }
        TreeKind::TypeApply { fun, .. } => method_and_clause(fun),
        _ => (fun.sym, 0),
    }
}

/// nsc `Symbol.kindString`, for the symbols a deprecation names.
fn kind_string(st: &crate::symbol::SymbolTable, sym: SymbolId) -> &'static str {
    let s = st.get(sym);
    match s.kind {
        SymKind::Module | SymKind::ModuleClass => "object",
        SymKind::Class if s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::INTERFACE) => "trait",
        SymKind::Class => "class",
        SymKind::TypeMember => "type",
        SymKind::Package => "package",
        SymKind::Method if s.flags.contains(Flags::ACCESSOR) && !s.flags.contains(Flags::MUTABLE) => "value",
        SymKind::Method => "method",
        SymKind::Term if s.flags.contains(Flags::LAZY) => "lazy value",
        SymKind::Term if s.flags.contains(Flags::MUTABLE) => "variable",
        SymKind::Term => "value",
        _ => "symbol",
    }
}

/// Our method type's first clause, as the dotted names of its parameter
/// classes.
fn param_names(st: &crate::symbol::SymbolTable, ty: &Type) -> Option<Vec<String>> {
    let Type::Method { paramss, .. } = ty else {
        return Some(Vec::new());
    };
    let first = paramss.first()?;
    first.iter().map(|p| type_name(st, p)).collect()
}

fn type_name(st: &crate::symbol::SymbolTable, t: &Type) -> Option<String> {
    Some(match t {
        Type::Int => "scala.Int".into(),
        Type::Long => "scala.Long".into(),
        Type::Short => "scala.Short".into(),
        Type::Byte => "scala.Byte".into(),
        Type::Char => "scala.Char".into(),
        Type::Float => "scala.Float".into(),
        Type::Double => "scala.Double".into(),
        Type::Boolean => "scala.Boolean".into(),
        Type::Unit => "scala.Unit".into(),
        Type::Class { sym, .. } => st.get(*sym).jvm_name.replace('/', "."),
        _ => return None,
    })
}

fn sig_param_names(t: &scala_rs_pickle::sym::SigType) -> Option<Vec<String>> {
    use scala_rs_pickle::sym::SigType;
    match t {
        SigType::Poly { result, .. } => sig_param_names(result),
        SigType::Method { params, .. } => params
            .iter()
            .map(|p| match &p.ty {
                SigType::Ref { sym, .. } => Some(sym.clone()),
                _ => None,
            })
            .collect(),
        _ => Some(Vec::new()),
    }
}

fn for_each_child(t: &Tree, f: &mut dyn FnMut(&Tree)) {
    match &t.kind {
        TreeKind::PackageDef { stats, .. } => stats.iter().for_each(f),
        TreeKind::LabelDef { rhs, .. } => f(rhs),
        TreeKind::Block { stats, expr } => {
            stats.iter().for_each(&mut *f);
            f(expr)
        }
        TreeKind::If { cond, thenp, elsep } => {
            f(cond);
            f(thenp);
            f(elsep)
        }
        TreeKind::Function { vparams, body } => {
            vparams.iter().for_each(&mut *f);
            f(body)
        }
        TreeKind::Assign { lhs, rhs } => {
            f(lhs);
            f(rhs)
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
            f(cond);
            f(body)
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => f(expr),
        TreeKind::InterpolatedString { args, .. } => args.iter().for_each(f),
        _ => {}
    }
}
