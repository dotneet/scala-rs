//! Java annotations in their class-file form.
//!
//! nsc writes an annotation whose class is a Java annotation interface into
//! the class file (`BCodeHelpers.emitAnnotations`), where reflection -- and
//! every library that reads `@Schema`, `@GET` or `@JsonProperty` at run time
//! -- finds it. A Scala annotation (`StaticAnnotation`) lives in the pickle
//! only. Which attribute it goes to is the annotation class's
//! `@Retention`: `SOURCE` is not written at all, `CLASS` goes to
//! `RuntimeInvisibleAnnotations`, and `RUNTIME` -- or no `@Retention`, as
//! scalac 2.13.16 reads it -- to `RuntimeVisibleAnnotations`.
//!
//! The typer turns each such annotation into a [`JavaAnnot`] while it
//! resolves the annotation's class, since the element values are read in the
//! annotation's scope: a constant, a Java enum constant, a `classOf[T]`, a
//! nested `new Anno(...)`, or an `Array(...)` of those, each against the type
//! the annotation interface declares for its element. The backend only
//! encodes the result. Where a definition's annotation lands -- field,
//! accessor, constructor parameter -- follows the meta-annotations written
//! with it (`@(A @field)`), kept in [`JavaAnnot::targets`].

use crate::check::Typer;
use scala_rs_parser::ast::*;

/// A Java annotation as the class file stores it.
#[derive(Clone, Debug, PartialEq)]
pub struct JavaAnnot {
    /// The annotation interface's descriptor (`Ljakarta/ws/rs/GET;`).
    pub desc: String,
    /// `RuntimeVisibleAnnotations` rather than `RuntimeInvisibleAnnotations`.
    pub visible: bool,
    /// The elements written, in source order.
    pub elems: Vec<(String, JavaAnnotValue)>,
    /// The meta-annotations written with it (`field`, `getter`, `setter`,
    /// `param`, `beanGetter`, `beanSetter`); empty for the default place.
    pub targets: Vec<String>,
    /// For a `@Repeatable` interface, its container's descriptor and
    /// visibility: nsc writes repeated annotations as one container
    /// annotation holding them all.
    pub container: Option<(String, bool)>,
}

impl JavaAnnot {
    /// Whether the annotation goes to `target` of a definition whose default
    /// place is `default`.
    pub fn applies_to(&self, target: &str, default: &str) -> bool {
        if self.targets.is_empty() {
            target == default
        } else {
            self.targets.iter().any(|t| t == target)
        }
    }
}

/// One `element_value` (JVMS §4.7.16.1).
#[derive(Clone, Debug, PartialEq)]
pub enum JavaAnnotValue {
    /// A primitive constant, with its tag (`B C I S Z` as `Int`, `J`, `F`, `D`).
    Int(u8, i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(String),
    /// A Java enum constant: the enum's descriptor and the constant's name.
    Enum {
        desc: String,
        name: String,
    },
    /// `classOf[T]`; the backend writes the erased type's descriptor.
    Class(Type),
    Annot(JavaAnnot),
    Array(Vec<JavaAnnotValue>),
}

/// What the class file of an annotation interface says about it.
#[derive(Clone)]
pub(crate) struct AnnotClassMeta {
    /// `None` for `@Retention(SOURCE)`: nothing is written.
    visible: Option<bool>,
    /// Each element's return descriptor.
    elems: rustc_hash::FxHashMap<String, String>,
    /// The container of a `@Repeatable` interface, as a descriptor.
    repeatable: Option<String>,
}

const ACC_ANNOTATION: u16 = 0x2000;
const ACC_ENUM: u16 = 0x4000;

impl Typer {
    /// The Java annotation `annot` stands for, when its class `ty` is a Java
    /// annotation interface that is not source-retained.
    pub(crate) fn java_annotation(&mut self, annot: &Tree, ty: &Type) -> Option<JavaAnnot> {
        let Type::Class { sym, .. } = ty else {
            return None;
        };
        let jvm = self.st.jvm_internal(*sym);
        let meta = self.annot_class_meta(&jvm)?;
        let visible = meta.visible?;
        let mut head = annot;
        let mut args: Vec<&Tree> = Vec::new();
        while let TreeKind::Apply { fun, args: xs } = &head.kind {
            args.splice(0..0, xs.iter());
            head = fun;
        }
        let mut targets = Vec::new();
        while let TreeKind::AnnotatedTypeTree { tpt, annot } = &head.kind {
            let path = annot.annotation_path();
            targets.push(path.rsplit('.').next().unwrap_or(&path).to_string());
            head = tpt;
        }
        let elems = self.java_annot_elems(&meta, &args)?;
        let container = meta.repeatable.as_ref().and_then(|desc| {
            let jvm = desc.strip_prefix('L')?.strip_suffix(';')?;
            let visible = self.annot_class_meta(jvm)?.visible?;
            Some((desc.clone(), visible))
        });
        Some(JavaAnnot {
            desc: format!("L{jvm};"),
            visible,
            elems,
            targets,
            container,
        })
    }

    fn java_annot_elems(
        &mut self,
        meta: &AnnotClassMeta,
        args: &[&Tree],
    ) -> Option<Vec<(String, JavaAnnotValue)>> {
        let mut out = Vec::new();
        for arg in args {
            let (name, rhs) = match &arg.kind {
                TreeKind::Assign { lhs, rhs } => match &lhs.kind {
                    TreeKind::Ident { name } => (name.clone(), rhs.as_ref()),
                    _ => return None,
                },
                _ => ("value".to_string(), *arg),
            };
            let Some(desc) = meta.elems.get(&name).cloned() else {
                self.error(
                    arg.span,
                    format!("unknown annotation argument name: {name}"),
                );
                return None;
            };
            let value = self.java_annot_value(rhs, &desc)?;
            out.push((name, value));
        }
        Some(out)
    }

    /// `t` as the value of an element declared with return descriptor `desc`.
    fn java_annot_value(&mut self, t: &Tree, desc: &str) -> Option<JavaAnnotValue> {
        if let Some(elem) = desc.strip_prefix('[') {
            let items = match array_elements(t) {
                Some(items) => items,
                // A single value for an array element, as Java allows.
                None => vec![t],
            };
            let mut values = Vec::new();
            for item in items {
                values.push(self.java_annot_value(item, elem)?);
            }
            return Some(JavaAnnotValue::Array(values));
        }
        match desc {
            "Ljava/lang/Class;" => {
                let targ = classof_type_arg(t).or_else(|| self.not_a_constant(t))?;
                let ty = self.with_strict_sig_names(|s| s.tree_to_type(targ));
                (!ty.is_error()).then_some(JavaAnnotValue::Class(ty))
            }
            "Ljava/lang/String;" => match self.annot_constant(t)? {
                Lit::String(s) => Some(JavaAnnotValue::Str(s)),
                _ => self.not_a_constant(t),
            },
            "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D" => {
                let lit = self.annot_constant(t)?;
                let tag = desc.as_bytes()[0];
                let value = convert_constant(&lit, tag);
                if value.is_none() {
                    return self.not_a_constant(t);
                }
                value
            }
            _ => {
                let jvm = desc.strip_prefix('L')?.strip_suffix(';')?;
                let access = self.binary_class_access(jvm)?;
                if access & ACC_ENUM != 0 {
                    let name = match &t.kind {
                        TreeKind::Select { name, .. } | TreeKind::Ident { name } => name.clone(),
                        _ => return self.not_a_constant(t),
                    };
                    return Some(JavaAnnotValue::Enum {
                        desc: desc.to_string(),
                        name,
                    });
                }
                if access & ACC_ANNOTATION != 0 {
                    let meta = self.annot_class_meta(jvm)?;
                    let mut head = t;
                    let mut args: Vec<&Tree> = Vec::new();
                    while let TreeKind::Apply { fun, args: xs } = &head.kind {
                        args.splice(0..0, xs.iter());
                        head = fun;
                    }
                    if !matches!(head.kind, TreeKind::New { .. }) {
                        return self.not_a_constant(t);
                    }
                    let elems = self.java_annot_elems(&meta, &args)?;
                    return Some(JavaAnnotValue::Annot(JavaAnnot {
                        desc: desc.to_string(),
                        visible: meta.visible.unwrap_or(true),
                        elems,
                        targets: Vec::new(),
                        container: None,
                    }));
                }
                self.not_a_constant(t)
            }
        }
    }

    /// The constant `t` denotes: a literal, a reference to a constant value,
    /// or a fold of those.
    fn annot_constant(&mut self, t: &Tree) -> Option<Lit> {
        match self.fold_constant(t) {
            Some(lit) => Some(lit),
            None => self.not_a_constant(t),
        }
    }

    fn fold_constant(&mut self, t: &Tree) -> Option<Lit> {
        match &t.kind {
            TreeKind::Literal { lit } => return Some(lit.clone()),
            TreeKind::Typed { expr, .. } => return self.fold_constant(expr),
            TreeKind::Select { qual, name } if name == "unary_-" => {
                return match self.fold_constant(qual)? {
                    Lit::Int(n) => Some(Lit::Int(n.wrapping_neg())),
                    Lit::Long(n) => Some(Lit::Long(n.wrapping_neg())),
                    Lit::Float(n) => Some(Lit::Float(-n)),
                    Lit::Double(n) => Some(Lit::Double(-n)),
                    _ => None,
                };
            }
            TreeKind::Apply { fun, args } if args.len() == 1 => {
                if let TreeKind::Select { qual, name } = &fun.kind {
                    if name == "+" {
                        let l = self.fold_constant(qual)?;
                        let r = self.fold_constant(&args[0])?;
                        return add_constants(&l, &r);
                    }
                }
            }
            _ => {}
        }
        let mark = self.diags.len();
        let mut typed = t.clone();
        self.type_expr(&mut typed, &Type::NoType);
        self.diags.truncate(mark);
        match &typed.ty {
            Type::Constant(lit) => Some(lit.clone()),
            _ => match &typed.kind {
                TreeKind::Literal { lit } => Some(lit.clone()),
                _ => None,
            },
        }
    }

    fn not_a_constant<T>(&mut self, t: &Tree) -> Option<T> {
        self.error(t.span, "annotation argument needs to be a constant");
        None
    }

    /// The class-file facts of annotation interface `jvm`, or `None` when it
    /// is not one.
    fn annot_class_meta(&mut self, jvm: &str) -> Option<AnnotClassMeta> {
        if let Some(known) = self.java_annot_classes.get(jvm) {
            return known.clone();
        }
        let meta = self
            .binary
            .find_class(jvm)
            .ok()
            .flatten()
            .and_then(|bytes| crate::javaclass::parse_java_classfile(&bytes).ok())
            .filter(|c| c.access & ACC_ANNOTATION != 0)
            .map(|c| AnnotClassMeta {
                visible: match c.retention.as_deref() {
                    Some("SOURCE") => None,
                    Some("CLASS") => Some(false),
                    _ => Some(true),
                },
                elems: c
                    .methods
                    .iter()
                    .filter(|m| m.desc.starts_with("()"))
                    .map(|m| (m.name.clone(), m.desc[2..].to_string()))
                    .collect(),
                repeatable: c.repeatable.clone(),
            });
        self.java_annot_classes
            .insert(jvm.to_string(), meta.clone());
        meta
    }

    fn binary_class_access(&mut self, jvm: &str) -> Option<u16> {
        let bytes = self.binary.find_class(jvm).ok().flatten()?;
        crate::javaclass::parse_java_classfile(&bytes)
            .ok()
            .map(|c| c.access)
    }
}

/// The elements of `Array(...)`, `Array[T](...)` or `Array.empty[T]`.
fn array_elements(t: &Tree) -> Option<Vec<&Tree>> {
    let is_array = |f: &Tree| match &f.kind {
        TreeKind::Ident { name } => name == "Array",
        TreeKind::Select { qual, name } => {
            name == "Array" || (name == "apply" && qual.annotation_path().ends_with("Array"))
        }
        TreeKind::TypeApply { fun, .. } => {
            matches!(&fun.kind, TreeKind::Ident { name } | TreeKind::Select { name, .. } if name == "Array" || name == "apply")
        }
        _ => false,
    };
    match &t.kind {
        TreeKind::Apply { fun, args } if is_array(fun) => Some(args.iter().collect()),
        TreeKind::TypeApply { .. } | TreeKind::Select { .. }
            if t.annotation_path().ends_with("Array.empty") =>
        {
            Some(Vec::new())
        }
        _ => None,
    }
}

/// The `T` of `classOf[T]`.
fn classof_type_arg(t: &Tree) -> Option<&Tree> {
    let TreeKind::TypeApply { fun, args } = &t.kind else {
        return None;
    };
    let is_classof = matches!(&fun.kind, TreeKind::Ident { name } | TreeKind::Select { name, .. } if name == "classOf");
    is_classof.then(|| args.first()).flatten()
}

/// `l + r` for constants.
fn add_constants(l: &Lit, r: &Lit) -> Option<Lit> {
    match (l, r) {
        (Lit::String(_), _) | (_, Lit::String(_)) => {
            Some(Lit::String(format!("{}{}", lit_text(l)?, lit_text(r)?)))
        }
        (Lit::Int(a), Lit::Int(b)) => Some(Lit::Int(a.wrapping_add(*b))),
        (Lit::Long(a), Lit::Long(b)) => Some(Lit::Long(a.wrapping_add(*b))),
        _ => None,
    }
}

/// A constant's text in a string concatenation.
fn lit_text(l: &Lit) -> Option<String> {
    Some(match l {
        Lit::String(s) => s.clone(),
        Lit::Int(n) => n.to_string(),
        Lit::Long(n) => n.to_string(),
        Lit::Boolean(b) => b.to_string(),
        Lit::Char(c) => c.to_string(),
        _ => return None,
    })
}

/// `lit` as an element of primitive type `tag`, as nsc's constant
/// conversion widens it.
fn convert_constant(lit: &Lit, tag: u8) -> Option<JavaAnnotValue> {
    let as_i64 = match lit {
        Lit::Int(n) => Some(*n as i64),
        Lit::Long(n) => Some(*n),
        Lit::Char(c) => Some(*c as i64),
        _ => None,
    };
    Some(match tag {
        b'Z' => match lit {
            Lit::Boolean(b) => JavaAnnotValue::Int(b'Z', i32::from(*b)),
            _ => return None,
        },
        b'C' => match lit {
            Lit::Char(c) => JavaAnnotValue::Int(b'C', *c as i32),
            Lit::Int(n) if u16::try_from(*n).is_ok() => JavaAnnotValue::Int(b'C', *n),
            _ => return None,
        },
        b'B' | b'S' | b'I' => match lit {
            Lit::Int(n) => JavaAnnotValue::Int(tag, *n),
            Lit::Char(c) => JavaAnnotValue::Int(tag, *c as i32),
            _ => return None,
        },
        b'J' => JavaAnnotValue::Long(as_i64?),
        b'F' => match lit {
            Lit::Float(f) => JavaAnnotValue::Float(*f),
            _ => JavaAnnotValue::Float(as_i64? as f32),
        },
        b'D' => match lit {
            Lit::Double(d) => JavaAnnotValue::Double(*d),
            Lit::Float(f) => JavaAnnotValue::Double(*f as f64),
            _ => JavaAnnotValue::Double(as_i64? as f64),
        },
        _ => return None,
    })
}
