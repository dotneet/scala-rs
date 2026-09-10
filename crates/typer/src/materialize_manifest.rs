//! Runtime evidence built with the library's Manifest factories. Full manifests
//! retain type arguments; an abstract argument requires evidence from scope.
use crate::check::Typer;
use crate::implicits::ImplicitSearch;
use crate::symbol::SymKind;
use scala_rs_parser::{NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

impl Typer {
    fn manifest_request(&self, pt: &Type) -> Option<(SymbolId, Type, bool)> {
        let Type::Class { sym, args } = pt else {
            return None;
        };
        if !self.library_abi || args.len() != 1 {
            return None;
        }
        match self.st.get(*sym).jvm_name.as_str() {
            "scala/reflect/Manifest" => Some((*sym, args[0].clone(), true)),
            "scala/reflect/OptManifest" => Some((*sym, args[0].clone(), false)),
            _ => None,
        }
    }

    // Use the same recursive evidence requirements when ranking an implicit
    // derivation as when building its arguments. Carry search depth through
    // this path so a recursive derivation cannot reset the divergence limit.
    pub(crate) fn manifest_available(&self, pt: &Type, depth: usize) -> bool {
        let Some((cls, t, full)) = self.manifest_request(pt) else {
            return false;
        };
        if depth > 32 {
            return false;
        }
        let sub = |t: &Type| {
            let want = Type::Class {
                sym: cls,
                args: vec![t.clone()],
            };
            match self.search_implicit_at(&want, depth + 1) {
                ImplicitSearch::Found(_) => true,
                ImplicitSearch::None => self.manifest_available(&want, depth + 1),
                ImplicitSearch::Ambiguous(_) => false,
            }
        };
        match &t {
            Type::Array(e) => !full || sub(e),
            Type::Class { sym, args } => self.manifest_static_class(*sym) && args.iter().all(sub),
            Type::Tuple(args) => args.iter().all(sub),
            Type::Function { params, ret } => params.iter().all(sub) && sub(ret),
            Type::Refined { parents, .. } if !full => {
                crate::erasure::intersection_dominator(parents, &self.st).is_some_and(|p| sub(&p))
            }
            Type::Refined { parents, .. } => !parents.is_empty() && parents.iter().all(sub),
            Type::TypeParam(_) | Type::TypeMember(_) | Type::Applied { .. } => !full,
            Type::SingleType { sym, .. } | Type::ModuleRef(sym) => {
                self.manifest_stable_reference(*sym)
            }
            Type::ThisType(sym) => self.manifest_this_available(*sym),
            Type::Annotated { tpe, .. } => sub(tpe),
            Type::BoundedWildcard { hi, .. } => sub(hi.as_deref().unwrap_or(&Type::Any)),
            Type::Wildcard
            | Type::Constant(_)
            | Type::Unit
            | Type::Boolean
            | Type::Byte
            | Type::Short
            | Type::Int
            | Type::Long
            | Type::Float
            | Type::Double
            | Type::Char
            | Type::String
            | Type::Any
            | Type::AnyRef
            | Type::AnyVal
            | Type::Null
            | Type::Nothing => true,
            _ => false,
        }
    }

    fn manifest_static_class(&self, sym: SymbolId) -> bool {
        let s = self.st.get(sym);
        s.kind == SymKind::Class && self.manifest_stable_reference(sym)
    }

    fn manifest_stable_reference(&self, sym: SymbolId) -> bool {
        // Source singleton types currently discard an instance selection's
        // prefix. Do not materialize that lost receiver as the caller's this.
        let mut owner = self.st.get(sym).owner;
        for _ in 0..64 {
            if owner.is_none() {
                return true;
            }
            match self.st.get(owner).kind {
                SymKind::Class => return false,
                SymKind::Package => return true,
                SymKind::Method => {
                    return self.st.lookup_term(&self.st.get(sym).name).contains(&sym)
                }
                _ => owner = self.st.get(owner).owner,
            }
        }
        false
    }

    fn manifest_this_available(&self, sym: SymbolId) -> bool {
        let mut owner = self.st.owner;
        for _ in 0..64 {
            if owner == sym {
                return true;
            }
            if owner.is_none() {
                return false;
            }
            owner = self.st.get(owner).owner;
        }
        false
    }

    fn no_manifest(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        let mut tree = self.manifest_module("scala.reflect.NoManifest", span)?;
        tree.ty = pt.clone();
        Some(tree)
    }

    pub(crate) fn manifest_fallback(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        self.manifest_build(pt, span, 0)
    }

    fn manifest_sub(&mut self, cls: SymbolId, t: &Type, span: Span, depth: usize) -> Option<Tree> {
        let want = Type::Class {
            sym: cls,
            args: vec![t.clone()],
        };
        self.warm_implicit_scope(&want);
        match self.search_implicit_at(&want, depth) {
            ImplicitSearch::Found(id) => Some(self.implicit_tree(id, &want, span, depth)),
            ImplicitSearch::None => self.manifest_build(&want, span, depth),
            ImplicitSearch::Ambiguous(_) => None,
        }
    }

    fn manifest_module(&mut self, name: &str, span: Span) -> Option<Tree> {
        let cls = self
            .pickle
            .ensure_class(&mut self.st, &mut self.binary, name, true)?;
        let module = self
            .st
            .get(self.st.get(cls).owner)
            .members
            .iter()
            .copied()
            .find(|&id| {
                self.st.get(id).kind == SymKind::Module && self.st.module_class_of(id) == cls
            })?;
        let mut tree = Tree::new(
            NodeId(0),
            span,
            TreeKind::Ident {
                name: self.st.get(module).name.clone(),
            },
        );
        tree.sym = module;
        tree.ty = Type::ModuleRef(cls);
        Some(tree)
    }

    fn manifest_factory(
        &mut self,
        full: bool,
        name: &str,
        args: Option<Vec<Tree>>,
        pt: &Type,
        span: Span,
    ) -> Option<Tree> {
        let factory = if full {
            "scala.reflect.ManifestFactory"
        } else {
            "scala.reflect.ClassManifestFactory"
        };
        let recv = self.manifest_module(factory, span)?;
        let owner = self.st.module_class_of(recv.sym);
        self.pickle
            .complete(&mut self.st, &mut self.binary, owner, name);
        let id = self.st.lookup_member(owner, name).into_iter().find(|&id| {
            let s = self.st.get(id);
            if s.kind != SymKind::Method { return args.is_none(); }
            let Some(args) = &args else { return !self.st.takes_value_params(id) };
            let Type::Method { paramss, .. } = &s.ty else { return false };
            let params = paramss.first().map(Vec::as_slice).unwrap_or(&[]);
            if name == "classType" {
                // These overloads differ in the leading Class / Manifest
                // parameter, not just arity. This builder supplies no prefix.
                if !matches!(params.first(), Some(Type::Class { sym, .. }) if self.st.get(*sym).jvm_name == "java/lang/Class") { return false; }
            }
            if matches!(params.last(), Some(Type::Repeated(_))) {
                args.len() + 1 >= params.len()
            } else { args.len() == params.len() }
        })?;
        let mut fun = Tree::new(
            NodeId(0),
            span,
            TreeKind::Select {
                qual: Box::new(recv),
                name: name.into(),
            },
        );
        fun.sym = id;
        fun.ty = self.st.get(id).ty.clone();
        if let Some(args) = args {
            let mut tree = Tree::new(
                NodeId(0),
                span,
                TreeKind::Apply {
                    fun: Box::new(fun),
                    args,
                },
            );
            tree.sym = id;
            tree.ty = pt.clone();
            Some(tree)
        } else {
            fun.ty = pt.clone();
            Some(fun)
        }
    }

    fn manifest_build(&mut self, pt: &Type, span: Span, depth: usize) -> Option<Tree> {
        let (cls, t, full) = self.manifest_request(pt)?;
        if depth > 32 {
            return None;
        }
        let canonical = match &t {
            Type::Byte => Some("Byte"),
            Type::Short => Some("Short"),
            Type::Int => Some("Int"),
            Type::Long => Some("Long"),
            Type::Float => Some("Float"),
            Type::Double => Some("Double"),
            Type::Char => Some("Char"),
            Type::Boolean => Some("Boolean"),
            Type::Unit => Some("Unit"),
            Type::Any => Some("Any"),
            Type::AnyRef => Some("Object"),
            Type::AnyVal => Some("AnyVal"),
            Type::Null => Some("Null"),
            Type::Nothing => Some("Nothing"),
            Type::Class { sym, .. } if *sym == self.st.object_sym => Some("Object"),
            _ => None,
        };
        if let Some(name) = canonical {
            return self.manifest_factory(true, name, None, pt, span);
        }
        match &t {
            Type::Annotated { tpe, .. } => return self.manifest_sub(cls, tpe, span, depth + 1),
            Type::Constant(lit) => {
                return self.manifest_sub(cls, &Type::lit_underlying(lit), span, depth + 1)
            }
            Type::Wildcard => return self.manifest_sub(cls, &Type::Any, span, depth + 1),
            Type::BoundedWildcard { hi, .. } => {
                return self.manifest_sub(cls, hi.as_deref().unwrap_or(&Type::Any), span, depth + 1)
            }
            Type::Array(elem) => {
                let arg = if full {
                    self.manifest_sub(cls, elem, span, depth + 1)?
                } else {
                    // Partial array manifests need a ClassTag for the element;
                    // OptManifest's NoManifest cannot describe its runtime class.
                    let ct =
                        self.reflect_class("scala.reflect.ClassTag", "scala/reflect/ClassTag")?;
                    let want = Type::Class {
                        sym: ct,
                        args: vec![(**elem).clone()],
                    };
                    self.warm_implicit_scope(&want);
                    match self.search_implicit_at(&want, depth + 1) {
                        ImplicitSearch::Found(id) => self.implicit_tree(id, &want, span, depth + 1),
                        ImplicitSearch::None => match self.classtag_apply_fallback(&want, span) {
                            Some(tag) => tag,
                            None => return self.no_manifest(pt, span),
                        },
                        ImplicitSearch::Ambiguous(_) => return None,
                    }
                };
                return self.manifest_factory(full, "arrayType", Some(vec![arg]), pt, span);
            }
            Type::SingleType { sym, .. } | Type::ModuleRef(sym) => {
                if !self.manifest_stable_reference(*sym) {
                    return None;
                }
                let value = self.ref_implicit(*sym, span);
                return self.manifest_factory(full, "singleType", Some(vec![value]), pt, span);
            }
            Type::ThisType(sym) => {
                if !self.manifest_this_available(*sym) {
                    return None;
                }
                let mut value = Tree::new(
                    NodeId(0),
                    span,
                    TreeKind::This {
                        qual: Some(self.st.get(*sym).name.clone()),
                    },
                );
                value.sym = *sym;
                value.ty = t.clone();
                return self.manifest_factory(full, "singleType", Some(vec![value]), pt, span);
            }
            Type::Refined { parents, .. } if !full => {
                let dom = crate::erasure::intersection_dominator(parents, &self.st)?;
                return self.manifest_sub(cls, &dom, span, depth + 1);
            }
            Type::Refined { parents, .. } if full && !parents.is_empty() => {
                if parents.len() == 1 {
                    return self.manifest_sub(cls, &parents[0], span, depth + 1);
                }
                let mut args = Vec::new();
                for p in parents {
                    args.push(self.manifest_sub(cls, p, span, depth + 1)?);
                }
                return self.manifest_factory(true, "intersectionType", Some(args), pt, span);
            }
            _ => {}
        }
        let class = match &t {
            Type::Class { sym, args } if self.manifest_static_class(*sym) => {
                Some((*sym, args.clone()))
            }
            Type::String => Some((self.st.string_sym, vec![])),
            Type::Tuple(args) => self
                .reflect_class(
                    &format!("scala.Tuple{}", args.len()),
                    &format!("scala/Tuple{}", args.len()),
                )
                .map(|s| (s, args.clone())),
            Type::Function { .. } => self.st.function_class_form(&t).and_then(|c| match c {
                Type::Class { sym, args } => Some((sym, args)),
                _ => None,
            }),
            _ => None,
        };
        if let Some((sym, targs)) = class {
            let mut class_arg = Tree::new(
                NodeId(0),
                span,
                TreeKind::Ident {
                    name: "$classOf".into(),
                },
            );
            class_arg.ty = Type::Class {
                sym,
                args: targs.clone(),
            };
            let mut args = vec![class_arg];
            for t in targs {
                args.push(self.manifest_sub(cls, &t, span, depth + 1)?);
            }
            return self.manifest_factory(full, "classType", Some(args), pt, span);
        }
        if !full
            && matches!(
                t,
                Type::TypeParam(_) | Type::TypeMember(_) | Type::Applied { .. }
            )
        {
            // This is the library's real OptManifest fallback, not an erased
            // full Manifest for an abstract type.
            return self.no_manifest(pt, span);
        }
        None
    }
}
