//! The warnings nsc's `refchecks` phase issues by default, over the typed
//! trees of a run without errors:
//!
//! * `a pure expression does nothing in statement position` (with the
//!   `; multiline expressions might/may require enclosing parentheses`
//!   clause) and `discarded pure expression does nothing` --
//!   `RefChecks.transform`'s `Block` and `Template` cases, with
//!   `TreeInfo.isPureExprForWarningPurposes`;
//! * `comparing values of types A and B using `==` will always yield
//!   true/false` -- the value-class half of `checkSensibleEquals`.
//!
//! Our typer does not rewrite a value discarded in a `Unit` position into
//! nsc's `{ e; () }` (codegen pops it instead), so the pass reconstructs
//! where nsc's `adapt` would have: the leaves (not blocks, ifs, matches or
//! trys, which pass the expected type on) of an expression typed against
//! `Unit` whose own type does not conform to `Unit`.

use crate::check::Typer;
use crate::symbol::{SymKind, SymbolTable};
use crate::warn_util::{point_of, warning_at};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Phase};

pub(crate) fn run(t: &mut Typer, units: &mut [(&mut Tree, usize)]) {
    let mut out = Vec::new();
    for (tree, file) in units.iter() {
        let Some(src) = t.sources.get(*file).cloned() else {
            continue;
        };
        let mut lint = Refchecks {
            st: &t.st,
            src: &src,
            file: *file,
            fatal: t.fatal_warnings,
            out: Vec::new(),
            meths: Vec::new(),
            in_args: false,
        };
        lint.tree(tree, false);
        out.extend(lint.out);
    }
    t.diags.extend(out);
}

struct Refchecks<'a> {
    st: &'a SymbolTable,
    src: &'a str,
    file: usize,
    fatal: bool,
    out: Vec<Diagnostic>,
    /// Each enclosing method's name and whether its result type is `Unit`
    /// (for `return`).
    meths: Vec<(String, bool)>,
    /// Visiting the arguments of an application: a function literal there
    /// takes its result type from the callee, whose signature ours may give
    /// as `A => Unit` where nsc's is `[U](A => U)`; see `Function` below.
    in_args: bool,
}

/// Whether `ty` conforms to `Unit` for `adapt`'s value discarding.
fn conforms_to_unit(ty: &Type) -> bool {
    match ty {
        Type::Unit | Type::Nothing | Type::Constant(Lit::Unit) => true,
        // Error/unknown types, and method types (an unapplied method is not
        // value-discarded, it is eta-expanded or reported): never warn.
        Type::Error | Type::NoType | Type::Method { .. } | Type::Overload(_) => true,
        Type::Annotated { tpe, .. } => conforms_to_unit(tpe),
        _ => false,
    }
}

fn is_unit_literal(t: &Tree) -> bool {
    matches!(t.kind, TreeKind::Literal { lit: Lit::Unit })
}

/// `TreeInfo.explicitlyUnit` / `hasExplicitUnit`: `(e: Unit)`.
fn has_explicit_unit(t: &Tree) -> bool {
    match &t.kind {
        TreeKind::Typed { tpt, .. } => {
            matches!(tpt.ty, Type::Unit) || matches!(&tpt.kind, TreeKind::Ident { name } if name == "Unit")
        }
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => has_explicit_unit(fun),
        _ => false,
    }
}

impl<'a> Refchecks<'a> {
    fn warn(&mut self, point: u32, end: u32, msg: String) {
        self.out.push(warning_at(
            self.file,
            point,
            end,
            msg,
            Phase::Refchecks,
            self.fatal,
        ));
    }

    fn warn_at_tree(&mut self, t: &Tree, msg: String) {
        let p = point_of(t, self.src);
        self.warn(p, t.span.hi.0, msg);
    }

    // --- symbols -----------------------------------------------------------

    fn sym_flags(&self, sym: SymbolId) -> Flags {
        self.st.get(sym).flags
    }

    /// nsc `Symbol.isStable`: a term that is not a `var`, not a by-name
    /// parameter, and not a method unless it is a (stable) accessor.
    fn is_stable(&self, sym: SymbolId) -> bool {
        if sym.is_none() {
            return false;
        }
        let s = self.st.get(sym);
        match s.kind {
            SymKind::Term => {
                !s.flags.contains(Flags::MUTABLE)
                    && !s.flags.contains(Flags::BYNAME)
                    && !matches!(s.ty, Type::ByName(_))
            }
            SymKind::Module | SymKind::ModuleClass | SymKind::Package => true,
            SymKind::Method => {
                s.flags.contains(Flags::ACCESSOR) && !s.flags.contains(Flags::MUTABLE)
            }
            _ => false,
        }
    }

    /// nsc `Symbol.isAccessor`: a getter or setter. Our fields are the
    /// `Term`s a class-like symbol owns; a library accessor is a `Method`
    /// carrying `ACCESSOR`.
    fn is_accessor(&self, sym: SymbolId) -> bool {
        if sym.is_none() {
            return false;
        }
        let s = self.st.get(sym);
        match s.kind {
            SymKind::Method => s.flags.contains(Flags::ACCESSOR),
            SymKind::Term => {
                !s.flags.contains(Flags::PARAM) && !s.owner.is_none() && {
                    let o = self.st.get(s.owner);
                    matches!(o.kind, SymKind::Class | SymKind::ModuleClass | SymKind::Module)
                }
            }
            _ => false,
        }
    }

    fn is_method(&self, sym: SymbolId) -> bool {
        !sym.is_none() && self.st.get(sym).kind == SymKind::Method
    }

    fn is_lazy(&self, sym: SymbolId) -> bool {
        !sym.is_none() && self.sym_flags(sym).contains(Flags::LAZY)
    }

    // --- TreeInfo ----------------------------------------------------------

    /// nsc's constant folder turns `1 + 1` into `Literal(2)` before
    /// refchecks sees it.
    fn is_folded_constant(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::Literal { lit } => !matches!(lit, Lit::Symbol(_)),
            TreeKind::Apply { fun, args } => match &fun.kind {
                TreeKind::Select { qual, name } => {
                    let unary = args.is_empty() && name.starts_with("unary_");
                    let binary = args.len() == 1 && is_foldable_binop(name);
                    (unary || binary)
                        && is_primitive_or_string(&qual.ty)
                        && self.is_folded_constant(qual)
                        && args.iter().all(|a| self.is_folded_constant(a))
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// `TreeInfo.isExprSafeToInline`.
    fn safe_to_inline(&self, t: &Tree) -> bool {
        if self.is_folded_constant(t) {
            return true;
        }
        match &t.kind {
            TreeKind::Empty | TreeKind::This { .. } | TreeKind::Super { .. } | TreeKind::Literal { .. } => true,
            TreeKind::Ident { .. } => self.is_stable(t.sym),
            TreeKind::Select { qual, .. } => {
                if let TreeKind::Literal { lit } = &qual.kind {
                    // `-5`, `5.toString`: any member of an AnyVal constant.
                    if !matches!(lit, Lit::String(_) | Lit::Null | Lit::Symbol(_)) && !t.sym.is_none() {
                        return true;
                    }
                }
                self.is_stable(t.sym) && self.safe_to_inline(qual)
            }
            TreeKind::TypeApply { fun, .. } => self.safe_to_inline(fun),
            TreeKind::Apply { fun, args } if args.is_empty() => {
                // `this()` / `super()` is nsc's `Select(This, <init>)`, and a
                // constructor is not stable.
                if matches!(fun.kind, TreeKind::This { .. } | TreeKind::Super { .. }) {
                    return false;
                }
                let fsym = fun_symbol(fun);
                self.is_method(fsym)
                    && !self.is_lazy(fsym)
                    && self.st.get(fsym).name != "<init>"
                    && !self.sym_flags(fsym).contains(Flags::CONSTRUCTOR)
                    && self.safe_to_inline(fun)
            }
            TreeKind::Typed { expr, .. } => self.safe_to_inline(expr),
            TreeKind::Block { stats, expr } => {
                stats.iter().all(|s| self.is_pure_def(s)) && self.safe_to_inline(expr)
            }
            _ => false,
        }
    }

    /// `TreeInfo.isPureDef`.
    fn is_pure_def(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::Empty
            | TreeKind::ClassDef { .. }
            | TreeKind::TypeDef { .. }
            | TreeKind::Import { .. }
            | TreeKind::DefDef { .. } => true,
            TreeKind::ValDef { mods, rhs, .. } => {
                !mods.flags.contains(Flags::MUTABLE) && self.safe_to_inline(rhs)
            }
            _ => false,
        }
    }

    /// `TreeInfo.isPureExprForWarningPurposes`.
    fn is_pure_for_warning(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::Typed { expr, .. } => self.is_pure_for_warning(expr),
            TreeKind::Function { .. } => true,
            TreeKind::Empty => false,
            TreeKind::Literal { lit: Lit::Unit } => false,
            _ => {
                if t.ty.is_error() {
                    return false;
                }
                let warnable_ref = match &t.kind {
                    TreeKind::Ident { .. } => self.is_accessor(t.sym),
                    TreeKind::Select { qual, .. } => {
                        self.safe_to_inline(qual) && self.is_accessor(t.sym)
                    }
                    _ => false,
                };
                let sym = t.sym;
                let warnable_symbol = sym.is_none() || {
                    let s = self.st.get(sym);
                    !(matches!(s.kind, SymKind::Module | SymKind::ModuleClass)
                        || s.flags.contains(Flags::LAZY)
                        || s.flags.contains(Flags::BYNAME)
                        || matches!(s.ty, Type::ByName(_)))
                };
                (self.safe_to_inline(t) || warnable_ref) && warnable_symbol
            }
        }
    }

    fn check_pure(&mut self, t: &Tree, msg: &str) {
        if !has_explicit_unit(t) && self.is_pure_for_warning(t) {
            self.warn_at_tree(t, msg.to_string());
        }
    }

    // --- traversal ---------------------------------------------------------

    /// Visit `t`, typed against `Unit` when `unit_pt`.
    fn tree(&mut self, t: &Tree, unit_pt: bool) {
        match &t.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.tree(s, false);
                }
            }
            TreeKind::ClassDef { mods, impl_, vparamss, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                let params: usize = vparamss
                    .iter()
                    .flatten()
                    .map(|p| match &p.kind {
                        TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::MUTABLE) => 3,
                        TreeKind::ValDef { mods, .. }
                            if mods.flags.contains(Flags::ACCESSOR)
                                || mods.flags.contains(Flags::CASE) =>
                        {
                            2
                        }
                        _ => 1,
                    })
                    .sum();
                self.template(impl_, params);
            }
            TreeKind::ModuleDef { mods, impl_, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                self.template(impl_, 0);
            }
            TreeKind::DefDef { mods, tpt, rhs, name, vparamss, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                let _ = vparamss;
                let unit = name != "<init>"
                    && (self.written_unit(tpt, t) || is_procedure_syntax(t, rhs, self.src));
                self.meths.push((name.clone(), unit));
                self.tree(rhs, unit);
                self.meths.pop();
            }
            TreeKind::ValDef { tpt, rhs, .. } => {
                let unit = !matches!(tpt.kind, TreeKind::Empty) && matches!(tpt.ty, Type::Unit);
                self.tree(rhs, unit);
            }
            TreeKind::LabelDef { rhs, .. } => self.tree(rhs, false),
            TreeKind::Block { stats, expr } if stats.is_empty() => self.tree(expr, unit_pt),
            TreeKind::Block { stats, expr } => self.block(stats, expr, unit_pt),
            TreeKind::If { cond, thenp, elsep } => {
                self.tree(cond, false);
                // A missing `else` types the `then` branch against `Unit`. The
                // parser gives the `()` it supplies the span of the token after
                // the `if`.
                let missing_else = is_unit_literal(elsep) && elsep.span.lo.0 >= t.span.hi.0;
                let then_unit = unit_pt || missing_else;
                self.tree(thenp, then_unit);
                self.tree(elsep, unit_pt);
            }
            TreeKind::Match { selector, cases } => {
                self.tree(selector, false);
                for c in cases {
                    self.tree(&c.pat, false);
                    self.tree(&c.guard, false);
                    self.tree(&c.body, unit_pt);
                }
            }
            TreeKind::Try { block, catches, finalizer } => {
                self.tree(block, unit_pt);
                for c in catches {
                    self.tree(&c.pat, false);
                    self.tree(&c.guard, false);
                    self.tree(&c.body, unit_pt);
                }
                self.tree(finalizer, true);
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
                self.tree(cond, false);
                self.tree(body, true);
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.tree(p, false);
                }
                let ret_unit =
                    !self.in_args && matches!(fn_result(&t.ty), Some(Type::Unit));
                let saved = std::mem::replace(&mut self.in_args, false);
                self.tree(body, ret_unit);
                self.in_args = saved;
            }
            TreeKind::Return { expr } => {
                let (name, unit) = self.meths.last().cloned().unwrap_or_default();
                if unit && !conforms_to_unit(&expr.ty) {
                    if let Some(tp) = nsc_type_string(expr) {
                        // `Typers.typedReturn`, a typer warning.
                        let d = warning_at(
                            self.file,
                            t.span.lo.0,
                            t.span.hi.0,
                            format!(
                                "enclosing method {name} has result type Unit: return value of type {tp} discarded"
                            ),
                            Phase::Typer,
                            self.fatal,
                        );
                        self.out.push(d);
                    }
                }
                self.tree(expr, unit);
            }
            TreeKind::Typed { expr, tpt } => {
                // `(e: Unit)` is an explicit discard: nsc does not warn.
                let _ = tpt;
                self.leaf_children(expr);
            }
            _ => {
                if unit_pt && !conforms_to_unit(&t.ty) {
                    // nsc's `adapt` makes this `{ t; () }`.
                    self.check_pure(t, "a pure expression does nothing in statement position");
                }
                self.leaf_children(t);
            }
        }
    }

    /// Whether a `def`'s written result type is `Unit`.
    fn written_unit(&self, tpt: &Tree, def: &Tree) -> bool {
        if matches!(tpt.kind, TreeKind::Empty) {
            return false;
        }
        match &def.ty {
            Type::Method { ret, .. } => matches!(**ret, Type::Unit),
            Type::Unit => true,
            _ => matches!(tpt.ty, Type::Unit),
        }
    }

    fn block(&mut self, stats: &[Tree], expr: &Tree, unit_pt: bool) {
        // nsc: `count` is 0 when the block ends in `()`, and the result
        // expression is the discarded one when it is `{ e; () }`.
        let adapted = unit_pt && is_adaptable_leaf(expr) && !conforms_to_unit(&expr.ty);
        let count: usize = if is_unit_literal(expr) { 0 } else { 1 };
        let multiline = stats.len() > 1 - count;
        let msg = if multiline {
            "a pure expression does nothing in statement position; multiline expressions might require enclosing parentheses"
        } else {
            "a pure expression does nothing in statement position"
        };
        for s in stats {
            self.check_pure(s, msg);
        }
        for s in stats {
            self.tree(s, false);
        }
        if adapted {
            self.check_pure(expr, "discarded pure expression does nothing");
            self.leaf_children(expr);
        } else {
            self.tree(expr, unit_pt);
        }
    }

    fn template(&mut self, impl_: &Template, param_members: usize) {
        // nsc's typed template: the constructor, every statement, and the
        // accessors a `val`/`var` adds.
        let mut len = 1 + param_members;
        for s in &impl_.body {
            len += match &s.kind {
                TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::MUTABLE) => 3,
                TreeKind::ValDef { .. } => 2,
                _ => 1,
            };
        }
        let msg = if len > 2 {
            "a pure expression does nothing in statement position; multiline expressions may require enclosing parentheses"
        } else {
            "a pure expression does nothing in statement position"
        };
        for s in &impl_.body {
            if !is_def_like(s) {
                self.check_pure(s, msg);
            }
        }
        for p in &impl_.parents {
            self.tree(p, false);
        }
        for s in &impl_.body {
            self.tree(s, false);
        }
    }

    /// The children of a tree that does not pass an expected type on.
    fn leaf_children(&mut self, t: &Tree) {
        match &t.kind {
            TreeKind::Apply { fun, args } => {
                self.check_sensible(t, fun, args);
                self.tree(fun, false);
                let saved = std::mem::replace(&mut self.in_args, true);
                for a in args {
                    self.tree(a, false);
                }
                self.in_args = saved;
            }
            TreeKind::TypeApply { fun, .. } => self.tree(fun, false),
            TreeKind::Select { qual, .. } => self.tree(qual, false),
            TreeKind::Assign { lhs, rhs } => {
                self.tree(lhs, false);
                self.tree(rhs, false);
            }
            TreeKind::Throw { expr } => self.tree(expr, false),
            TreeKind::New { .. } => {}
            TreeKind::Typed { expr, .. } => self.tree(expr, false),
            TreeKind::InterpolatedString { args, .. } => {
                for a in args {
                    self.tree(a, false);
                }
            }
            TreeKind::Block { .. }
            | TreeKind::If { .. }
            | TreeKind::Match { .. }
            | TreeKind::Try { .. }
            | TreeKind::While { .. }
            | TreeKind::DoWhile { .. }
            | TreeKind::Function { .. }
            | TreeKind::Return { .. }
            | TreeKind::ClassDef { .. }
            | TreeKind::ModuleDef { .. }
            | TreeKind::DefDef { .. }
            | TreeKind::ValDef { .. } => self.tree(t, false),
            _ => {}
        }
    }

    // --- checkSensible -----------------------------------------------------

    /// `RefChecks.checkSensible` for `==` / `!=` between values of primitive
    /// types, `Unit`, `Null` and `String` -- the cases whose verdict does not
    /// depend on user-defined `equals` or on printing a user type.
    fn check_sensible(&mut self, tree: &Tree, fun: &Tree, args: &[Tree]) {
        let TreeKind::Select { qual, name } = &fun.kind else {
            return;
        };
        if !matches!(name.as_str(), "==" | "!=") || args.len() != 1 {
            return;
        }
        if !self.is_any_comparison(fun.sym) {
            return;
        }
        let other = strip_box(&args[0]);
        let (Some(recv), Some(act)) = (simple_value_kind(&qual.ty), simple_value_kind(&other.ty)) else {
            return;
        };
        let eq = name == "==";
        let verdict: Option<bool> = match (recv, act) {
            (ValueKind::Null, ValueKind::Null) => Some(true),
            (ValueKind::Null, a) | (a, ValueKind::Null) => {
                if a.is_primitive() {
                    Some(false)
                } else {
                    None
                }
            }
            (ValueKind::Boolean, a) => {
                if a != ValueKind::Boolean {
                    Some(false)
                } else {
                    None
                }
            }
            (ValueKind::Unit, a) => Some(a == ValueKind::Unit),
            (r, a) if r.is_numeric() => {
                if !a.is_numeric() {
                    Some(false)
                } else {
                    None
                }
            }
            (ValueKind::String, a) => {
                if a != ValueKind::String {
                    Some(false)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(always_equal) = verdict {
            let msg = always_equal == eq;
            let what = format!("values of types {} and {}", recv.name(), act.name());
            let point = point_of(tree, self.src);
            self.warn(
                point,
                tree.span.hi.0,
                format!("comparing {what} using `{name}` will always yield {msg}"),
            );
        }
    }

    /// `Any.==` / `Any.!=` / `AnyRef.==` (not a numeric overload).
    fn is_any_comparison(&self, sym: SymbolId) -> bool {
        if sym.is_none() {
            return false;
        }
        let owner = self.st.get(sym).owner;
        owner == self.st.any_sym || owner == self.st.anyref_sym || {
            let o = self.st.get(owner);
            o.jvm_name == "java/lang/Object" || o.name == "Any" || o.name == "AnyRef"
        }
    }
}

fn is_def_like(t: &Tree) -> bool {
    {
        matches!(
            t.kind,
            TreeKind::ValDef { .. }
                | TreeKind::DefDef { .. }
                | TreeKind::ClassDef { .. }
                | TreeKind::ModuleDef { .. }
                | TreeKind::TypeDef { .. }
                | TreeKind::Import { .. }
        )
    }
}

/// Whether nsc would adapt this result expression itself (rather than
/// passing `Unit` on to its branches).
fn is_adaptable_leaf(t: &Tree) -> bool {
    !matches!(
        t.kind,
        TreeKind::Block { .. }
            | TreeKind::If { .. }
            | TreeKind::Match { .. }
            | TreeKind::Try { .. }
            | TreeKind::Typed { .. }
            | TreeKind::While { .. }
            | TreeKind::DoWhile { .. }
            | TreeKind::Return { .. }
            | TreeKind::Throw { .. }
    )
}

fn fun_symbol(fun: &Tree) -> SymbolId {
    match &fun.kind {
        TreeKind::TypeApply { fun, .. } => fun_symbol(fun),
        _ => fun.sym,
    }
}

fn fn_result(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Function { ret, .. } => Some(ret),
        _ => None,
    }
}

fn is_foldable_binop(name: &str) -> bool {
    matches!(
        name,
        "+" | "-" | "*" | "/" | "%" | "<<" | ">>" | ">>>" | "&" | "|" | "^" | "==" | "!=" | "<"
            | ">" | "<=" | ">=" | "&&" | "||"
    )
}

fn is_primitive_or_string(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::Long
            | Type::Short
            | Type::Byte
            | Type::Char
            | Type::Float
            | Type::Double
            | Type::Boolean
            | Type::String
            | Type::Constant(_)
    )
}

/// Our typer boxes a primitive argument of `==(Any)` explicitly.
fn strip_box(t: &Tree) -> &Tree {
    if let TreeKind::Apply { fun, args } = &t.kind {
        if args.len() == 1 {
            if let TreeKind::Ident { name } = &fun.kind {
                if name == "$box" {
                    return &args[0];
                }
            }
        }
    }
    t
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueKind {
    Unit,
    Boolean,
    Byte,
    Short,
    Char,
    Int,
    Long,
    Float,
    Double,
    String,
    Null,
}

impl ValueKind {
    fn is_numeric(self) -> bool {
        matches!(
            self,
            ValueKind::Byte
                | ValueKind::Short
                | ValueKind::Char
                | ValueKind::Int
                | ValueKind::Long
                | ValueKind::Float
                | ValueKind::Double
        )
    }
    fn is_primitive(self) -> bool {
        !matches!(self, ValueKind::String | ValueKind::Null)
    }
    fn name(self) -> &'static str {
        match self {
            ValueKind::Unit => "Unit",
            ValueKind::Boolean => "Boolean",
            ValueKind::Byte => "Byte",
            ValueKind::Short => "Short",
            ValueKind::Char => "Char",
            ValueKind::Int => "Int",
            ValueKind::Long => "Long",
            ValueKind::Float => "Float",
            ValueKind::Double => "Double",
            ValueKind::String => "String",
            ValueKind::Null => "Null",
        }
    }
}

/// `def f(...) { ... }` (no `=`, no result type): nsc's procedure syntax, a
/// `Unit` method. (Our parser leaves the result type to inference.)
pub(crate) fn is_procedure_syntax(def: &Tree, rhs: &Tree, src: &str) -> bool {
    let TreeKind::DefDef { tpt, name, .. } = &def.kind else {
        return false;
    };
    if !matches!(tpt.kind, TreeKind::Empty) || name == "<init>" || rhs.span.is_dummy() {
        return false;
    }
    let lo = def.span.lo.0 as usize;
    let hi = rhs.span.lo.0 as usize;
    if lo >= hi || hi > src.len() || !src.is_char_boundary(lo) || !src.is_char_boundary(hi) {
        return false;
    }
    let mut before = src[lo..hi].trim_end();
    if !src[hi..].starts_with('{') {
        // `{ e }` is the expression `e` itself (nsc's `makeBlock`).
        match before.strip_suffix('{') {
            Some(b) => before = b.trim_end(),
            None => return false,
        }
    }
    !before.ends_with('=')
}

/// nsc's `Type.toString` for the types a warning may print; `None` for any
/// other, and the caller then does not warn rather than print a wrong type.
fn nsc_type_string(t: &Tree) -> Option<String> {
    if let TreeKind::Literal { lit } = &t.kind {
        // Typed without an expected type, a literal keeps its constant type.
        return Some(match lit {
            Lit::Int(n) => format!("Int({n})"),
            Lit::Long(n) => format!("Long({n}L)"),
            Lit::Boolean(b) => format!("Boolean({b})"),
            Lit::String(s) if !s.contains(['"', '\\', '\n']) => format!("String(\"{s}\")"),
            Lit::Char(c) if c.is_ascii_graphic() => format!("Char('{c}')"),
            Lit::Null => "Null(null)".to_string(),
            _ => return None,
        });
    }
    Some(
        match &t.ty {
            Type::Int => "Int",
            Type::Long => "Long",
            Type::Short => "Short",
            Type::Byte => "Byte",
            Type::Char => "Char",
            Type::Float => "Float",
            Type::Double => "Double",
            Type::Boolean => "Boolean",
            Type::String => "String",
            Type::Null => "Null",
            _ => return None,
        }
        .to_string(),
    )
}

/// The widened type as one of the kinds `check_sensible` decides on.
fn simple_value_kind(ty: &Type) -> Option<ValueKind> {
    Some(match ty {
        Type::Unit => ValueKind::Unit,
        Type::Boolean => ValueKind::Boolean,
        Type::Byte => ValueKind::Byte,
        Type::Short => ValueKind::Short,
        Type::Char => ValueKind::Char,
        Type::Int => ValueKind::Int,
        Type::Long => ValueKind::Long,
        Type::Float => ValueKind::Float,
        Type::Double => ValueKind::Double,
        Type::String => ValueKind::String,
        Type::Null => ValueKind::Null,
        Type::Constant(lit) => match lit {
            Lit::Unit => ValueKind::Unit,
            Lit::Boolean(_) => ValueKind::Boolean,
            Lit::Int(_) => ValueKind::Int,
            Lit::Long(_) => ValueKind::Long,
            Lit::Float(_) => ValueKind::Float,
            Lit::Double(_) => ValueKind::Double,
            Lit::Char(_) => ValueKind::Char,
            Lit::String(_) => ValueKind::String,
            Lit::Null => ValueKind::Null,
            Lit::Symbol(_) => return None,
        },
        _ => return None,
    })
}
