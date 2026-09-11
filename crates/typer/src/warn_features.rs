//! The reflective-call feature warning for members of an anonymous class.
//!
//! nsc gives `val t = new T { def foo = 3 }` the refinement type
//! `T{def foo: Int}`, so `t.foo` is a structural call, made by reflection,
//! and warned about unless `scala.language.reflectiveCalls` is enabled. Our
//! typer keeps the anonymous class's own type, so the call is found here: a
//! member the anonymous class declares (and does not inherit), selected
//! through a value whose type was inferred as that class. `new T { def bar
//! = 5 }.bar` is no structural call in nsc either.

use crate::check::Typer;
use crate::symbol::SymKind;
use crate::warn_util::{each_child, point_of};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Phase, WarnCategory};

pub(crate) fn run(t: &mut Typer, units: &mut [(&mut Tree, usize)]) {
    if t.language_reflective_calls {
        return;
    }
    let mut found: Vec<(usize, u32, String)> = Vec::new();
    for (tree, file) in units.iter() {
        let Some(src) = t.sources.get(*file).cloned() else {
            continue;
        };
        let mut v = Finder {
            t: &*t,
            src: &src,
            file: *file,
            out: &mut found,
        };
        v.visit(tree);
    }
    for (file, point, desc) in found {
        let fq = "scala.language.reflectiveCalls";
        let explain = if t.reported_features.insert("reflectiveCalls".into()) {
            format!(
                "\nThis can be achieved by adding the import clause 'import {fq}'\nor by setting the compiler option -language:reflectiveCalls.\nSee the Scaladoc for value {fq} for a discussion\nwhy the feature should be explicitly enabled."
            )
        } else {
            String::new()
        };
        let msg = format!(
            "{desc} should be enabled\nby making the implicit value {fq} visible.{explain}"
        );
        let span = scala_rs_span::Span::new(point, point + 1);
        let d = if t.fatal_warnings {
            Diagnostic::error(file, span, msg)
        } else {
            Diagnostic::warning(file, span, msg).with_category(WarnCategory::Feature)
        };
        t.diags.push(d.in_phase(Phase::Refchecks));
    }
}

struct Finder<'a> {
    t: &'a Typer,
    src: &'a str,
    file: usize,
    out: &'a mut Vec<(usize, u32, String)>,
}

impl<'a> Finder<'a> {
    /// A value or parameterless method whose type was inferred as an
    /// anonymous class.
    fn anon_class_of(&self, qual: &Tree) -> Option<SymbolId> {
        if !matches!(qual.kind, TreeKind::Ident { .. } | TreeKind::Select { .. })
            || qual.sym.is_none()
        {
            return None;
        }
        let s = self.t.st.get(qual.sym);
        if !matches!(s.kind, SymKind::Term | SymKind::Method) {
            return None;
        }
        let ty = match &qual.ty {
            Type::Method { paramss, ret } if paramss.is_empty() => &**ret,
            t => t,
        };
        let Type::Class { sym, .. } = ty else {
            return None;
        };
        let c = self.t.st.get(*sym);
        (c.kind == SymKind::Class && c.name.starts_with("$anon")).then_some(*sym)
    }

    fn visit(&mut self, t: &Tree) {
        if let TreeKind::Select { qual, name } = &t.kind {
            if let Some(cls) = self.anon_class_of(qual) {
                if !t.sym.is_none() && self.t.st.get(t.sym).owner == cls {
                    let st = &self.t.st;
                    let inherited = st
                        .get(cls)
                        .parents
                        .iter()
                        .filter_map(|p| st.class_sym_of(p))
                        .any(|p| {
                            st.members_including_inherited(p)
                                .iter()
                                .any(|m| st.get(*m).name == *name)
                        });
                    if !inherited {
                        let s = st.get(t.sym);
                        let kind = if s.kind == SymKind::Term && !s.flags.contains(Flags::MUTABLE) {
                            "value"
                        } else if s.kind == SymKind::Term {
                            "variable"
                        } else {
                            "method"
                        };
                        let desc =
                            format!("reflective access of structural type member {kind} {name}");
                        self.out.push((self.file, point_of(t, self.src), desc));
                    }
                }
            }
        }
        each_child(t, &mut |c| self.visit(c));
    }
}
