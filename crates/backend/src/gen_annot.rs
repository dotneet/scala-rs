//! Where a definition's Java annotations land in the class file.
//!
//! The typer leaves each Java annotation on the symbol it was written on
//! ([`scala_rs_typer::java_annot::JavaAnnot`]); this module encodes them and
//! places them as nsc does, which was measured with `javap -v` on scalac
//! 2.13.16 output:
//!
//! - a class, trait or object carries its own; an object's also go on its
//!   mirror class when that is not a companion class;
//! - a method carries its own, and each parameter its own, in the
//!   `Runtime*ParameterAnnotations` attribute; a static forwarder copies the
//!   module method's (see [`crate::companion_fwd::Forwarder`]);
//! - a primary-constructor parameter's annotation goes to the constructor
//!   parameter unless a meta-annotation (`@(A @field)`, `@(A @getter)`)
//!   sends it to the field or the accessor;
//! - a template `val` or `var`'s goes to the field, or with `@getter` /
//!   `@setter` to the accessor.

use crate::classfile::{encode_method_name, Annotation, ElementValue, ACC_BRIDGE, ACC_STATIC};
use crate::gen::*;
use crate::gen_desc::jvm_desc_val;
use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind};
use scala_rs_typer::java_annot::{JavaAnnot, JavaAnnotValue};
use scala_rs_typer::SymbolTable;

fn encode(st: &SymbolTable, a: &JavaAnnot) -> Annotation {
    Annotation {
        desc: a.desc.clone(),
        visible: a.visible,
        elems: a
            .elems
            .iter()
            .map(|(n, v)| (n.clone(), encode_value(st, v)))
            .collect(),
    }
}

fn encode_value(st: &SymbolTable, v: &JavaAnnotValue) -> ElementValue {
    match v {
        JavaAnnotValue::Int(tag, n) => ElementValue::Int(*tag, *n),
        JavaAnnotValue::Long(n) => ElementValue::Long(*n),
        JavaAnnotValue::Float(f) => ElementValue::Float(*f),
        JavaAnnotValue::Double(d) => ElementValue::Double(*d),
        JavaAnnotValue::Str(s) => ElementValue::Str(s.clone()),
        JavaAnnotValue::Enum { desc, name } => ElementValue::Enum {
            desc: desc.clone(),
            name: name.clone(),
        },
        // `classOf[Unit]` is `BoxedUnit.class` here, as in a value position.
        JavaAnnotValue::Class(ty) => ElementValue::Class(jvm_desc_val(st, ty)),
        JavaAnnotValue::Annot(a) => ElementValue::Annot(encode(st, a)),
        JavaAnnotValue::Array(items) => {
            ElementValue::Array(items.iter().map(|i| encode_value(st, i)).collect())
        }
    }
}

/// `annots` encoded, with a `@Repeatable` annotation written more than once
/// folded into its container where the first one stood, as nsc writes
/// `@Parameter(...) @Parameter(...)` as `@Parameters({...})`.
fn encode_all<'a>(
    st: &SymbolTable,
    annots: impl Iterator<Item = &'a JavaAnnot>,
) -> Vec<Annotation> {
    let annots: Vec<&JavaAnnot> = annots.collect();
    let mut out: Vec<Annotation> = Vec::new();
    let mut folded: Vec<String> = Vec::new();
    for a in &annots {
        let repeats: Vec<&&JavaAnnot> = annots.iter().filter(|b| b.desc == a.desc).collect();
        match &a.container {
            Some((container, visible)) if repeats.len() > 1 => {
                if folded.contains(&a.desc) {
                    continue;
                }
                folded.push(a.desc.clone());
                let items = repeats
                    .iter()
                    .map(|b| ElementValue::Annot(encode(st, b)))
                    .collect();
                out.push(Annotation {
                    desc: container.clone(),
                    visible: *visible,
                    elems: vec![("value".to_string(), ElementValue::Array(items))],
                });
            }
            _ => out.push(encode(st, a)),
        }
    }
    out
}

/// The annotations of `sym` that go to `target`, for a definition whose
/// unqualified annotations go to `default`.
pub(crate) fn annots_at(
    st: &SymbolTable,
    sym: SymbolId,
    target: &str,
    default: &str,
) -> Vec<Annotation> {
    if sym.is_none() {
        return Vec::new();
    }
    encode_all(
        st,
        st.get(sym)
            .java_annots
            .iter()
            .filter(|a| a.applies_to(target, default)),
    )
}

/// Every annotation of `sym`, wherever the meta-annotations would send it:
/// for a class or a method there is only the one place.
pub(crate) fn all_annots(st: &SymbolTable, sym: SymbolId) -> Vec<Annotation> {
    if sym.is_none() {
        return Vec::new();
    }
    encode_all(st, st.get(sym).java_annots.iter())
}

/// The parameter annotations of `vparamss`, placed `offset` slots into a
/// method whose descriptor is `desc` (a leading `$outer` or `$this`), with
/// an entry for every descriptor parameter so reflection never has to guess
/// which ones the list skips.
fn param_annots(
    st: &SymbolTable,
    params: impl Iterator<Item = SymbolId>,
    desc: &str,
    offset: usize,
) -> Vec<Vec<Annotation>> {
    let count = crate::companion_fwd::desc_slots(desc).map_or(0, |s| s.0.len());
    let mut out = vec![Vec::new(); count];
    for (i, p) in params.enumerate() {
        if let Some(slot) = out.get_mut(offset + i) {
            *slot = annots_at(st, p, "param", "param");
        }
    }
    out
}

impl ClassBuilder {
    /// Gives the method added last the annotations of the source method
    /// `def`: its own unless `params_only`, and its parameters', which start
    /// `offset` parameters into the descriptor.
    pub(crate) fn annotate_last_def(
        &mut self,
        st: &SymbolTable,
        def: &Tree,
        offset: usize,
        params_only: bool,
    ) {
        let TreeKind::DefDef { vparamss, .. } = &def.kind else {
            return;
        };
        if !params_only {
            self.add_java_annots_to_last(&all_annots(st, def.sym));
        }
        let Some(desc) = self.methods.last().map(|m| m.desc.clone()) else {
            return;
        };
        let params = vparamss.iter().flatten().map(|p| p.sym);
        self.set_param_annots_of_last(param_annots(st, params, &desc, offset));
    }

    /// Annotations of the primary constructor's parameters `vparamss` on
    /// the constructor at `index`.
    pub(crate) fn annotate_ctor_params(
        &mut self,
        st: &SymbolTable,
        index: MethodIndex,
        vparamss: &[Vec<Tree>],
        offset: usize,
    ) {
        let Some(desc) = self.methods.get(index.0).map(|m| m.desc.clone()) else {
            return;
        };
        let params = vparamss.iter().flatten().map(|p| p.sym);
        let annots = param_annots(st, params, &desc, offset);
        if annots.iter().any(|a| !a.is_empty()) {
            self.methods[index.0].param_annots = annots;
        }
    }

    /// The annotations a mixed-in trait `val` (`sym`) gives the field `name`
    /// the implementing class stores it in; the trait itself carries none.
    pub(crate) fn annotate_mixin_field(&mut self, st: &SymbolTable, name: &str, sym: SymbolId) {
        let annots = annots_at(st, sym, "field", "field");
        if !annots.is_empty() {
            self.annots.fields.insert(name.to_string(), annots);
        }
    }

    /// The class's own annotations, and those of the fields and accessors
    /// of its constructor parameters `vparamss` and template `body`. Run
    /// once every member is emitted, since fields and accessors come from
    /// many emitters.
    pub(crate) fn annotate_template(
        &mut self,
        st: &SymbolTable,
        class_id: SymbolId,
        vparamss: &[Vec<Tree>],
        body: &[Tree],
    ) {
        for a in all_annots(st, class_id) {
            if !self.annots.class.iter().any(|b| b.desc == a.desc) {
                self.annots.class.push(a);
            }
        }
        // A plain constructor parameter that code outside the constructor
        // reads is stored in a field, and nsc annotates that field as if it
        // were the parameter's default place too.
        let is_case = !class_id.is_none() && st.get(class_id).flags.contains(Flags::CASE);
        let ctor_params = vparamss.iter().enumerate().flat_map(|(clause, ps)| {
            ps.iter().map(move |p| {
                let flags = match &p.kind {
                    TreeKind::ValDef { mods, .. } => mods.flags,
                    _ => Flags::default(),
                };
                let plain = !flags.contains(Flags::ACCESSOR)
                    && !flags.contains(Flags::MUTABLE)
                    && !(is_case && clause == 0);
                (p, if plain { "field" } else { "param" })
            })
        });
        let vals = body.iter().filter_map(|t| match &t.kind {
            TreeKind::ValDef { .. } => Some((t, "field")),
            _ => None,
        });
        for (vd, default) in ctor_params.chain(vals) {
            let TreeKind::ValDef { name, .. } = &vd.kind else {
                continue;
            };
            if vd.sym.is_none() || st.get(vd.sym).java_annots.is_empty() {
                continue;
            }
            let field = annots_at(st, vd.sym, "field", default);
            if !field.is_empty() && self.fields.iter().any(|f| f.name == *name) {
                self.annots.fields.insert(name.clone(), field);
            }
            let getter = annots_at(st, vd.sym, "getter", default);
            let setter = annots_at(st, vd.sym, "setter", default);
            let getter_name = encode_method_name(name);
            let setter_name = encode_method_name(&format!("{name}_="));
            for m in &mut self.methods {
                if m.access & (ACC_BRIDGE | ACC_STATIC) != 0 {
                    continue;
                }
                let annots = if m.name == getter_name && m.desc.starts_with("()") {
                    &getter
                } else if m.name == setter_name && m.desc.ends_with(")V") {
                    &setter
                } else {
                    continue;
                };
                for a in annots {
                    if !m.java_annots.iter().any(|b| b.desc == a.desc) {
                        m.java_annots.push(a.clone());
                    }
                }
            }
        }
    }
}
