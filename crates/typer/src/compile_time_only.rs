//! `@scala.annotation.compileTimeOnly(message)`.
//!
//! nsc's `RefChecks.checkUndesiredProperties`: a reference that survives
//! type checking to a symbol carrying the annotation is an error whose text
//! is the annotation's message, reported at the reference -- unless the
//! reference sits inside a definition that is itself annotated (the
//! placeholder's own implementation, or the conversion an annotated
//! `implicit class` desugars to). Two kinds of reference are checked, as in
//! nsc:
//!
//! * a *term* reference (an identifier or selection naming the symbol), read
//!   off the typed trees once every unit has been typed
//!   ([`Typer::check_compile_time_only`]);
//! * a *written type* (`val v: (C7, C7)`, `new C1`, `List.empty[C7]`, `x:
//!   @placebo`, `@placebo def x`), noted as the type tree is resolved
//!   ([`Typer::note_cto_ref`]), because type trees keep no symbol of their
//!   own. An inferred type is never a reference.
//!
//! An annotated *case* class is reported at its definition, as nsc does:
//! its companion's `apply` and `unapply` name the class, and they are not
//! inside it. A call to that `apply` is a reference like any other.
//!
//! Only source definitions carry annotations in this compiler (pickled
//! members do not), so the library's own placeholders are unaffected; the
//! whole check is off unless some source mentions the annotation.

use crate::check::Typer;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

pub(crate) fn is_cto_path(path: &str) -> bool {
    matches!(
        path,
        "compileTimeOnly" | "annotation.compileTimeOnly" | "scala.annotation.compileTimeOnly"
    )
}

/// The message of a `@compileTimeOnly(…)` annotation tree: its string
/// argument, with `"a" + "b"` folded.
fn literal_message(t: &Tree) -> Option<String> {
    match &t.kind {
        TreeKind::Literal {
            lit: Lit::String(s),
        } => Some(s.clone()),
        TreeKind::Apply { fun, args } if args.len() == 1 => match &fun.kind {
            TreeKind::Select { qual, name } if name == "+" || name == "$plus" => {
                Some(literal_message(qual)? + &literal_message(&args[0])?)
            }
            _ => None,
        },
        _ => None,
    }
}

impl Typer {
    /// The `@compileTimeOnly` message on `sym`, if it carries one.
    pub(crate) fn cto_message(&self, sym: SymbolId) -> Option<String> {
        if sym.is_none() {
            return None;
        }
        let s = self.st.get(sym);
        let a = s
            .annotations
            .iter()
            .find(|a| is_cto_path(&a.annotation_path()))?;
        let msg = match &a.kind {
            TreeKind::Apply { args, .. } => args.first().and_then(literal_message),
            _ => None,
        };
        Some(msg.unwrap_or_else(|| {
            format!(
                "Reference to {} should not have survived past type checking,\n\
                 it should have been processed and eliminated during expansion of an enclosing macro.",
                s.name
            )
        }))
    }

    /// Whether `sym` or a definition enclosing it is annotated.
    fn cto_owner_chain(&self, mut sym: SymbolId) -> bool {
        let mut steps = 0;
        while !sym.is_none() && steps < 64 {
            if self.cto_message(sym).is_some() {
                return true;
            }
            let up = self.st.get(sym).owner;
            if up == sym {
                break;
            }
            sym = up;
            steps += 1;
        }
        false
    }

    /// A written type tree resolved to `sym`: report it unless the
    /// definition being typed is (inside) an annotated one.
    pub(crate) fn note_cto_ref(&mut self, sym: SymbolId, span: Span) {
        if !self.any_cto || self.header_pass || sym.is_none() {
            return;
        }
        let Some(msg) = self.cto_message(sym) else {
            return;
        };
        if self.cto_owner_chain(self.st.owner)
            || self.cto_owner_chain(self.st.this_class)
            || self.return_meth.is_some_and(|m| self.cto_owner_chain(m))
            || self.cto_owner_chain(self.macro_lexical_owner)
            || self.cto_owner_chain(self.cto_sig_owner)
        {
            return;
        }
        if let Some(held) = self.cto_deferred.as_mut() {
            held.push((span, msg));
            return;
        }
        self.error(span, msg);
    }

    /// A report held back by `type_apply`'s `List[T]()` rule that turned out
    /// not to apply: pass it on (to an enclosing hold, if any).
    pub(crate) fn note_cto_held(&mut self, span: Span, msg: String) {
        match self.cto_deferred.as_mut() {
            Some(held) => held.push((span, msg)),
            None => self.error(span, msg),
        }
    }

    /// The class a simple annotation or type name resolves to, without
    /// reporting anything when it does not resolve.
    /// `at` is where nsc reports it: the annotated definition, or the
    /// annotation itself in a type (`x: @placebo`).
    pub(crate) fn note_cto_type_name(&mut self, annot: &Tree, at: Span) {
        if !self.any_cto {
            return;
        }
        let head = match &annot.kind {
            TreeKind::Apply { fun, .. } => fun.as_ref(),
            _ => annot,
        };
        let TreeKind::Ident { name } = &head.kind else {
            return;
        };
        if let Some(&sym) = self.st.lookup_type(name).first() {
            self.note_cto_ref(sym, at);
        }
    }

    /// `List[T]()` with no arguments: nsc's refchecks turns it into `Nil`
    /// before looking at the type arguments, so `List[C7]()` is not a
    /// reference to `C7` while `List.empty[C7]` is.
    pub(crate) fn is_list_nil_shape(tree: &Tree) -> bool {
        let TreeKind::Apply { fun, args } = &tree.kind else {
            return false;
        };
        if !args.is_empty() {
            return false;
        }
        let TreeKind::TypeApply { fun: inner, .. } = &fun.kind else {
            return false;
        };
        let names_list = |t: &Tree| matches!(&t.kind, TreeKind::Ident { name } | TreeKind::Select { name, .. } if name == "List");
        match &inner.kind {
            TreeKind::Select { qual, name } if name == "apply" => names_list(qual),
            _ => names_list(inner),
        }
    }

    /// Whether a typed `List[T]()` really called `immutable.List.apply`.
    pub(crate) fn is_list_module_apply(&self, tree: &Tree) -> bool {
        let TreeKind::Apply { fun, .. } = &tree.kind else {
            return false;
        };
        let TreeKind::TypeApply { fun: inner, .. } = &fun.kind else {
            return false;
        };
        let sym = if inner.sym.is_none() {
            fun.sym
        } else {
            inner.sym
        };
        if sym.is_none() {
            return false;
        }
        let owner = self.st.get(sym).owner;
        let jvm = |id: SymbolId| self.st.get(id).jvm_name.clone();
        jvm(sym) == "scala/collection/immutable/List$"
            || (!owner.is_none() && jvm(owner) == "scala/collection/immutable/List$")
    }

    /// The term references of one typed unit.
    pub(crate) fn check_compile_time_only(&mut self, tree: &Tree) {
        if !self.any_cto {
            return;
        }
        let mut found: Vec<(Span, String)> = Vec::new();
        self.cto_walk(tree, false, &mut found);
        for (span, msg) in found {
            self.error(span, msg);
        }
    }

    fn cto_walk(&self, t: &Tree, inside: bool, out: &mut Vec<(Span, String)>) {
        let own = match &t.kind {
            TreeKind::ClassDef { .. }
            | TreeKind::ModuleDef { .. }
            | TreeKind::DefDef { .. }
            | TreeKind::ValDef { .. }
            | TreeKind::TypeDef { .. } => self.cto_message(t.sym).is_some(),
            _ => false,
        };
        if let TreeKind::ClassDef { mods, .. } = &t.kind {
            if own && !inside && mods.flags.contains(Flags::CASE) {
                if let Some(msg) = self.cto_message(t.sym) {
                    out.push((t.span, msg));
                }
            }
        }
        let inside = inside || own;
        if !inside {
            if let TreeKind::Ident { .. } | TreeKind::Select { .. } = &t.kind {
                if let Some(msg) = self.cto_term_ref(t.sym) {
                    out.push((t.span, msg));
                }
            }
        }
        let mut kids: Vec<&Tree> = Vec::new();
        crate::macros::push_children(t, &mut kids);
        for k in kids {
            self.cto_walk(k, inside, out);
        }
    }

    /// The message a term reference to `sym` reports: the symbol's own, or
    /// for the synthetic companion `apply` / `unapply` of an annotated case
    /// class, the class's.
    fn cto_term_ref(&self, sym: SymbolId) -> Option<String> {
        if sym.is_none() {
            return None;
        }
        let s = self.st.get(sym);
        // A class, type or package symbol on an identifier is a type tree the
        // compiler built (an implicit class's conversion writes `new C(x)`),
        // not a term reference; written types are checked as they resolve.
        if matches!(
            s.kind,
            SymKind::TypeParam | SymKind::Package | SymKind::Class | SymKind::TypeMember
        ) {
            return None;
        }
        if let Some(m) = self.cto_message(sym) {
            return Some(m);
        }
        // The companion `apply` the namer synthesizes for a case class
        // (`synthesize_case_members`): it builds the class it returns.
        if s.kind == SymKind::Method
            && s.name == "apply"
            && s.flags.contains(Flags::SYNTHETIC)
            && s.flags.contains(Flags::CASE)
        {
            if let Type::Method { ret, .. } = &s.ty {
                if let Type::Class { sym: cls, .. } = ret.as_ref() {
                    if self.st.get(*cls).flags.contains(Flags::CASE) {
                        return self.cto_message(*cls);
                    }
                }
            }
        }
        None
    }
}
