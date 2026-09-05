#![allow(dead_code)]
//! Typing a template -- `class`, `trait`, `object`, and the type members
//! declared in one -- and the rules a finished template has to satisfy.
//!
//! The first half builds the body: self types, inherited members brought into
//! scope, type aliases expanded, anonymous classes and eta expansions. The
//! second half is the rule checks that run once a template is complete:
//! overriding, variance, abstract members, `@tailrec`, stored annotations and
//! the messages for an implicit that was not found.

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;
use std::collections::HashSet;

impl Typer {
    // ------------------------------------------------------------------ typer
    pub(crate) fn typer(&mut self, tree: &mut Tree) {
        match &mut tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                self.st.push_scope();
                if !tree.sym.is_none() {
                    for m in self.st.get(tree.sym).members.clone() {
                        let n = self.st.get(m).name.clone();
                        if n.ends_with('$') {
                            continue;
                        }
                        self.st.enter_in_current(&n, m);
                    }
                }
                for s in stats.iter_mut() {
                    self.typer(s);
                }
                self.st.pop_scope();
            }
            TreeKind::ClassDef { .. } => self.type_class(tree),
            TreeKind::ModuleDef { .. } => self.type_module(tree),
            TreeKind::Import { .. } => self.type_import(tree),
            _ => {
                self.type_stat(tree);
            }
        }
    }

    pub(crate) fn type_anon_class(&mut self, tpt: &mut Tree) {
        if let TreeKind::ClassDef { name, impl_, .. } = &mut tpt.kind {
            if name == "$anon" {
                self.gensym += 1;
                *name = format!("$anon${}", self.gensym);
            }
            if impl_.parents.is_empty() {
                let mut p = Tree::dummy(TreeKind::Ident {
                    name: "AnyRef".into(),
                });
                p.span = tpt.span;
                impl_.parents.push(p);
            }
        }
        if tpt.sym.is_none() {
            self.namer_class(tpt);
        }
        self.type_class(tpt);
    }

    pub(crate) fn type_eta(&mut self, tree: &mut Tree, pt: &Type) {
        let dummy_method = Type::Method {
            paramss: vec![],
            ret: Box::new(Type::NoType),
        };
        if let TreeKind::Typed { expr, .. } = &mut tree.kind {
            self.type_expr(expr, &dummy_method);
        }
        if let TreeKind::Typed { expr, .. } = &mut tree.kind {
            let inner = std::mem::replace(expr, Box::new(Tree::dummy(TreeKind::Empty)));
            *tree = *inner;
        }
        if let Type::Method { paramss, ret } = tree.ty.clone() {
            // One lambda per parameter list: `curry(1) _` on
            // `def curry(a: Int)(b: Int)(c: Int)` is `Int => Int => Int`, not
            // a flattened `(Int, Int) => Int`.
            let mut clauses: Vec<Vec<Type>> = if paramss.is_empty() {
                vec![Vec::new()]
            } else {
                paramss
            };
            let mut ret = *ret;
            // `xs.map(identity _)` names a polymorphic method: its own type
            // parameters are the expected function type's to solve, exactly as
            // for the `_`-less form.
            if clauses.len() == 1 {
                let (ps, r) = self.solve_eta_tparams(tree.sym, clauses[0].clone(), ret.clone(), pt);
                clauses[0] = ps;
                ret = r;
            }
            crate::uncurry::eta_expand_curried(&mut self.st, &mut self.gensym, tree, &clauses, ret);
        }
    }

    pub(crate) fn type_class(&mut self, tree: &mut Tree) {
        let id = tree.sym;
        if let TreeKind::ClassDef { mods, vparamss, .. } = &tree.kind {
            if mods.flags.contains(Flags::IMPLICIT) {
                let owner_kind = if !id.is_none() {
                    self.st.get(self.st.get(id).owner).kind
                } else {
                    self.st.get(self.st.owner).kind
                };
                if owner_kind == SymKind::Package {
                    // nsc: top-level `implicit class` is illegal. Package objects
                    // own the class (ModuleClass), so they do not take this path.
                    self.error(
                        tree.span,
                        "`implicit` modifier cannot be used for top-level objects",
                    );
                }
                let nparams = vparamss.first().map(|c| c.len()).unwrap_or(0);
                if nparams != 1 {
                    self.error(
                        tree.span,
                        "unimplemented: implicit class must have a single parameter",
                    );
                }
                // scalac 2.13.16 does not warn for an `implicit class` even
                // under `-feature`; only an implicit conversion method does.
            }
        }
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        let saved_ret = self.return_meth;
        self.st.owner = id;
        self.st.this_class = id;
        self.return_meth = None;
        self.st.push_scope();
        // re-enter members into local scope
        for m in self.st.get(id).members.clone() {
            let n = self.st.get(m).name.clone();
            self.st.enter_in_current(&n, m);
        }
        let (self_name, self_tpt, is_trait) = match &tree.kind {
            TreeKind::ClassDef { impl_, mods, .. } => (
                impl_.self_name.clone(),
                impl_.self_tpt.clone(),
                mods.flags.contains(Flags::TRAIT),
            ),
            _ => (None, None, false),
        };
        let tree_span = tree.span;
        let (vparamss, body, parents, tparams) = match &mut tree.kind {
            TreeKind::ClassDef {
                vparamss,
                impl_,
                tparams,
                ..
            } => (
                vparamss,
                &mut impl_.body,
                &mut impl_.parents,
                tparams.clone(),
            ),
            _ => return,
        };
        // The namer entered these parameters before the unit's `import`
        // clauses were processed, so a bound naming an imported type could not
        // resolve there. The class scope is now complete (the parameters
        // themselves are members, re-entered above) and the imports are in
        // scope, so this is where the bounds get their real types -- and where
        // an unresolvable one is finally reported.
        self.resolve_tparam_bounds(&tparams);
        // The signature pass and the body pass walk the same tree; the
        // evidence clause must be appended by only one of them.
        let ev_fresh = tree.id == scala_rs_parser::NodeId(0)
            || self.sig_done.insert((self.file_index, tree.id));
        let evidence = if is_trait || !ev_fresh {
            Vec::new()
        } else {
            self.class_bound_evidence(id, &tparams)
        };
        if !evidence.is_empty() {
            vparamss.push(evidence);
        }
        // Ctor params must be typed before `extends C(z)` so the argument `z`
        // is a known term, not a NoType ident.
        let mut paramss_ty: Vec<Vec<Type>> = Vec::new();
        let mut paramss_ids: Vec<Vec<SymbolId>> = Vec::new();
        let mut all_ctor_params = Vec::new();
        for clause in vparamss.iter_mut() {
            let mut ct = Vec::new();
            let mut ids = Vec::new();
            for p in clause.iter_mut() {
                self.type_val_sig(p);
                ct.push(p.ty.clone());
                if !p.sym.is_none() {
                    // The constructor takes `T*`; the field it becomes holds a
                    // `Seq[T]`, which is what the class body sees.
                    let field_ty = match &p.ty {
                        Type::Repeated(elem) => self.seq_of(elem).unwrap_or_else(|| p.ty.clone()),
                        other => other.clone(),
                    };
                    self.st.get_mut(p.sym).ty = field_ty;
                    // `type_val_sig` sets the `DEFAULTPARAM` flag but (unlike
                    // `type_def_sig` for ordinary methods) never captures the
                    // default value tree itself — a ctor param default
                    // (`class Foo(x: Int, y: Int = 5)`) would otherwise never
                    // get filled in at a `new Foo(1)` call site.
                    if let TreeKind::ValDef { mods, rhs, .. } = &p.kind {
                        if mods.flags.contains(Flags::DEFAULTPARAM) && !rhs.is_empty() {
                            self.st.get_mut(p.sym).default_rhs = Some((**rhs).clone());
                            self.record_default_scope(p.sym, false);
                        }
                    }
                    ids.push(p.sym);
                    all_ctor_params.push(p.sym);
                }
            }
            paramss_ty.push(ct);
            paramss_ids.push(ids);
        }
        // Primary `<init>` type must be visible before auxiliary `this(...)` bodies.
        if !id.is_none() {
            for mem in self.st.get(id).members.clone() {
                if self.st.get(mem).name == "<init>"
                    && self.st.get(mem).params == self.st.get(id).ctor_fields.clone()
                {
                    self.st.get_mut(mem).params = all_ctor_params.clone();
                    self.st.get_mut(mem).paramss = paramss_ids.clone();
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: paramss_ty.clone(),
                        ret: Box::new(Type::Unit),
                    };
                    // Unlike an ordinary method's `name$default$N` getters, a
                    // constructor default can't be an instance method on the
                    // class being constructed (there is no receiver yet at
                    // `new Foo(1)`; nsc emits these on the companion instead).
                    // `default_getter_apply`'s receiver logic assumes an
                    // existing instance, so it doesn't fit constructors —
                    // deliberately not calling `synthesize_default_getters`
                    // here. `fill_defaults_and_implicits` still fills the
                    // omitted arg via the raw `default_rhs` fallback above,
                    // typed directly at the call site; this covers simple
                    // defaults (literals, `null`, ...) but not one referring
                    // to an earlier ctor param, which would need real
                    // companion-based getters (not implemented here).
                }
            }
            // Nothing in *this* run calls a constructor's default getter, but a
            // separately compiled caller does, so the companion module still
            // owes the method. See `crate::ctor_defaults`.
            self.synthesize_ctor_default_getters(id, &paramss_ids);
        }
        // `copy`'s parameter symbols (allocated in `synthesize_case_members`,
        // during the namer pass, before ctor param types are known) still hold
        // `Type::NoType` until now. Re-sync them from the just-resolved field
        // types, then (re)build `copy$default$N` — this is also the first time
        // `synthesize_default_getters` runs for `copy`, since doing it any
        // earlier would have baked in the same `NoType` placeholders.
        if !id.is_none() && self.st.get(id).flags.contains(Flags::CASE) {
            let all_ctor_param_tys: Vec<Type> = paramss_ty.iter().flatten().cloned().collect();
            if let Some(copy_id) = self.st.get(id).members.iter().copied().find(|&m| {
                self.st.get(m).kind == SymKind::Method
                    && self.st.get(m).name == "copy"
                    && self.st.get(m).flags.contains(Flags::SYNTHETIC)
            }) {
                let copy_params = self.st.get(copy_id).params.clone();
                if copy_params.len() == all_ctor_param_tys.len() {
                    for (pid, ty) in copy_params.iter().zip(all_ctor_param_tys.iter()) {
                        self.st.get_mut(*pid).ty = ty.clone();
                    }
                    // `copy` must be curried exactly like the constructor it
                    // mirrors: a case class with a second (or later) ctor
                    // parameter list (`case class TableNode(a, b)(val
                    // profileTable: Any)`, slick's `ast/Node.scala`) has a
                    // `copy` nsc curries the same way. Flattening every list
                    // into one made `t.copy(identity = x)(t.profileTable)`
                    // (real, curried, two calls) type as `t.copy(identity =
                    // x)` alone returning a `TableNode` already, so the
                    // trailing `(t.profileTable)` read as `TableNode`'s own
                    // `apply` — "value apply is not a member of TableNode".
                    let mut copy_paramss: Vec<Vec<SymbolId>> = Vec::with_capacity(paramss_ty.len());
                    let mut rest = copy_params.as_slice();
                    for group in &paramss_ty {
                        let (head, tail) = rest.split_at(group.len());
                        copy_paramss.push(head.to_vec());
                        rest = tail;
                    }
                    self.st.get_mut(copy_id).paramss = copy_paramss.clone();
                    self.st.get_mut(copy_id).ty = Type::Method {
                        paramss: paramss_ty.clone(),
                        ret: Box::new(Type::Class {
                            sym: id,
                            args: vec![],
                        }),
                    };
                    // Deliberately *not* copying `copy`'s access onto the
                    // `copy$default$N` getters, which scalac 2.13.16 does make
                    // private under `-Xsource-features:case-apply-copy-access`
                    // (`javap -p`: `private int copy$default$1();`). Nothing
                    // in Scala source can name them, and this compiler fills
                    // an omitted `copy` argument at the call site rather than
                    // through the getter, so `ACC_PRIVATE` here would only
                    // risk an `IllegalAccessError` for no observable gain.
                    // See docs/not-implemented.md.
                    self.synthesize_default_getters(id, copy_id, "copy", &[], &copy_paramss);
                }
            }
        }
        let mut pts = Vec::new();
        let saved_parent_ctx = self.parent_ctx.replace((id, saved_this));
        for p in parents.iter_mut() {
            self.type_parent(p);
            pts.push(p.ty.clone());
        }
        self.parent_ctx = saved_parent_ctx;
        if !pts.is_empty() {
            self.st.get_mut(id).parents = pts;
        }
        // `class X extends Loud` where `trait Loud extends Animal` really *is*
        // an `Animal` (SLS 5.1): the trait's superclass becomes X's. Splice it
        // in as a parent so member lookup, `super` and the emitted
        // `invokespecial Animal.<init>` all see it — without this the class
        // file extended `java/lang/Object` and every use as an `Animal`
        // failed the verifier.
        // Not during the header (`sigs_only`) pass: there a trait's own parents
        // are still the unapplied `Type::Class { args: [] }` that
        // `parents_pass_tmpl` installs, so the spliced parent would come out as
        // a bare `StatementInvoker` and the real pass would then reject it with
        // `StatementInvoker takes type parameters`.
        let inherited_super = if self.sigs_only {
            None
        } else {
            crate::traitparent::inherited_superclass(&self.st, id)
        };
        if let Some(kty) = inherited_super {
            let k = self.st.class_sym_of(&kty).unwrap_or(SymbolId::NONE);
            let span = parents.first().map(|p| p.span).unwrap_or(tree_span);
            let mut t = Tree::new(
                NodeId(0),
                span,
                TreeKind::Ident {
                    name: self.st.get(k).name.clone(),
                },
            );
            t.ty = kty.clone();
            t.sym = k;
            parents.insert(0, t);
            let mut ps = self.st.get(id).parents.clone();
            ps.insert(0, kty.clone());
            self.st.get_mut(id).parents = ps;
            // The class writes no argument list, so the trait's superclass
            // must be constructible without one.
            if !k.is_none() {
                let targs: Vec<Type> = match &kty {
                    Type::Class { args, .. } => args.clone(),
                    _ => Vec::new(),
                };
                if matches!(self.pick_ctor_at(k, &targs, &[], None), OverloadPick::None) {
                    let name = self.st.get(k).name.clone();
                    self.error(span, format!("no matching overload for constructor {name}"));
                }
            }
        }
        self.check_mixin_superclasses(id, parents, tree_span);
        self.link_case_product(id);
        self.register_sealed_child(id);
        self.enter_inherited_members(id);
        self.bind_self_type(id, self_name, self_tpt.as_deref());
        self.presig_import_prefixes(body);
        let saved_missed = self.import_prefix_missed;
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::Import { .. }) {
                self.type_import(stt);
            }
        }
        // type aliases / abstract type members before other signatures
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_type_member(stt);
            }
        }
        self.finish_type_aliases(body);
        // After every member of the template has a bound, so that a cycle
        // spread over two of them (`type X <: Y; type Y <: X`) is visible.
        let member_bounds: Vec<(SymbolId, Span)> = body
            .iter()
            .filter(|s| matches!(s.kind, TreeKind::TypeDef { .. }) && !s.sym.is_none())
            .map(|s| (s.sym, s.span))
            .collect();
        self.report_bound_cycles(&member_bounds);
        self.st.get_mut(id).ty = Type::Class {
            sym: id,
            args: vec![],
        };
        for stt in body.iter_mut() {
            if !matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_member_sig_deferrable(stt);
            }
        }
        self.import_prefix_missed = saved_missed;
        if !self.sigs_only {
            for stt in body.iter_mut() {
                self.type_member_body(stt);
            }
        }
        self.finish_case_apply(id, &paramss_ty, &paramss_ids);
        self.check_type_member_kind_override(id, tree.span);
        self.check_self_conformance(id, tree.span);
        self.check_mixin_parents(id, tree.span);
        self.check_class_variance(id, tree.span);
        // A value class erases to what it wraps, so one that wraps another
        // value class has no erasure at all and unfolds for ever. nsc rejects
        // the shape instead of trying (`neg/t5878`, `neg/t10530`).
        if let Some(msg) = crate::cyclic::value_class_wraps_value_class(&self.st, id) {
            self.error(tree.span, msg);
        }
        if !self.sigs_only {
            // SLS 5.1.7 / SIP-15. Only on the body pass: a value class nested
            // in a method body is never reached by the signature pass, so
            // running it there as well would only add duplicates for
            // `dedup_diags` to remove.
            for v in crate::valueclass::violations(
                &self.st, id, tree_span, is_trait, vparamss, &tparams, body,
            ) {
                self.error(v.span, v.msg);
            }
        }
        if !self.sigs_only {
            let body_snapshot: Vec<Tree> = body.to_vec();
            self.check_abstract_override_placement(id, &body_snapshot);
            // `new C with T` reaches here as the `$anon` class the parser
            // built; scalac reports that one as `object creation impossible.`
            let anon = self.st.get(id).name.starts_with("$anon");
            let headline = if anon {
                "object creation impossible.".to_string()
            } else {
                format!("class {} needs to be a mixin.", self.st.get(id).name)
            };
            self.check_abstract_override_grounded(id, tree_span, &headline);
            self.check_overrides(id, &body_snapshot, tree_span);
            self.check_double_defs(id, &body_snapshot);
            let missing_headline = if anon {
                "object creation impossible.".to_string()
            } else {
                format!("class {} needs to be abstract.", self.st.get(id).name)
            };
            self.check_missing_implementations(id, tree_span, &missing_headline);
        }
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        self.return_meth = saved_ret;
        tree.ty = Type::Class {
            sym: id,
            args: vec![],
        };
    }

    /// Fill in the signatures of the `apply` / `unapply` that
    /// `synthesize_case_members` allocated for a case class, now that the
    /// constructor's parameter types are known. Only those synthetic members
    /// are touched: a companion may also declare `apply`/`unapply` of its own
    /// (`object LiteralNode { def apply(tp: Type, v: Any, vol: Boolean = false) }`),
    /// and overwriting those with the constructor's signature would hide them.
    ///
    /// `unapply` extracts from the *first* parameter section only (nsc: extra
    /// curried sections, e.g. `case class F(name: String)(val opts: X)`, are
    /// not part of the pattern); `apply` mirrors every section, curried,
    /// exactly like the primary constructor.
    fn finish_case_apply(
        &mut self,
        class_id: SymbolId,
        paramss_ty: &[Vec<Type>],
        paramss_ids: &[Vec<SymbolId>],
    ) {
        if class_id.is_none() || !self.st.get(class_id).flags.contains(Flags::CASE) {
            return;
        }
        // The constructor's parameter types are only known here, and they are
        // the `AbstractFunctionN` the companion extends.
        self.link_case_companion(class_id, paramss_ty);
        let ctor_param_tys = paramss_ty.first().cloned().unwrap_or_default();
        let name = self.st.get(class_id).name.clone();
        let companion = self
            .st
            .lookup(&name)
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Module);
        if let Some(m) = companion {
            let cls = match self.st.get(m).ty {
                Type::ModuleRef(c) => c,
                _ => m,
            };
            // nsc synthesizes `apply[A](a: A): Box[A]`, so `Box(1)` infers
            // `A`. The class's own parameters stand in for the method's.
            let tps = self.st.get(class_id).tparams.clone();
            let class_ty = Type::Class {
                sym: class_id,
                args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
            };
            let unapply_ret = match ctor_param_tys.len() {
                0 => Type::Boolean,
                1 => Type::Class {
                    sym: self.st.option_sym,
                    args: vec![ctor_param_tys[0].clone()],
                },
                _ => Type::Class {
                    sym: self.st.option_sym,
                    args: vec![Type::Tuple(ctor_param_tys.clone())],
                },
            };
            for mem in self.st.get(cls).members.clone() {
                if !self.st.get(mem).flags.contains(Flags::SYNTHETIC) {
                    continue;
                }
                let n = self.st.get(mem).name.clone();
                if n == "apply" {
                    // `apply` is a *method*, and nsc gives it parameters of its
                    // own. Handing it the class's meant one symbol stood for
                    // both, and a call made from inside the class substituted
                    // `U := U`: the parameter still mentioned a type parameter
                    // of the callee, which is what "undetermined" is read from,
                    // so the argument was checked against the *bound*.
                    // `case class SV[T, U](a: T, b: Bx[U]) { def f = SV(1, b) }`
                    // reported `found: Bx[U]  required: Bx[Any]` -- and so did
                    // slick's `ShapedValue.packedValue`.
                    let own = self.fresh_method_tparams(mem, &tps);
                    let to: Vec<Type> = own.iter().map(|t| Type::TypeParam(*t)).collect();
                    let paramss: Vec<Vec<Type>> = paramss_ty
                        .iter()
                        .map(|ps| {
                            ps.iter()
                                .map(|t| crate::symbol::subst_tparams_slice(&tps, &to, t))
                                .collect()
                        })
                        .collect();
                    let ret = crate::symbol::subst_tparams_slice(&tps, &to, &class_ty);
                    self.st.get_mut(mem).tparams = own;
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss,
                        ret: Box::new(ret),
                    };
                    self.st.get_mut(mem).paramss = paramss_ids.to_vec();
                    self.st.get_mut(mem).params = paramss_ids.iter().flatten().copied().collect();
                } else if n == "unapply" {
                    self.st.get_mut(mem).tparams = tps.clone();
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: vec![vec![class_ty.clone()]],
                        ret: Box::new(unapply_ret.clone()),
                    };
                }
            }
        }
        // Primary constructor only. Auxiliary `def this` already has a Method
        // type from `type_def_sig`; overwriting it with the primary arity would
        // make `new C(1)` and `extends C(1)` miss the aux overload.
        for mem in self.st.get(class_id).members.clone() {
            if self.st.get(mem).name == "<init>" && self.st.get(mem).ty.is_no_type() {
                self.st.get_mut(mem).ty = Type::Method {
                    paramss: paramss_ty.to_vec(),
                    ret: Box::new(Type::Unit),
                };
            }
        }
    }

    /// Fresh type parameters for `owner`, mirroring `src`'s names, kinds and
    /// bounds. A synthesized method (a case class's companion `apply`) needs
    /// its own; reusing the class's makes one symbol both "fixed here" and
    /// "still to be inferred at the call", and nothing downstream can tell the
    /// two apart. Variance is *not* copied: a method's type parameters have
    /// none, and carrying `+T` over would let the variance check read them.
    fn fresh_method_tparams(&mut self, owner: SymbolId, src: &[SymbolId]) -> Vec<SymbolId> {
        if src.is_empty() {
            return Vec::new();
        }
        let out: Vec<SymbolId> = src
            .iter()
            .map(|tp| {
                let name = self.st.get(*tp).name.clone();
                let id = self
                    .st
                    .alloc(&name, owner, SymKind::TypeParam, Flags::EMPTY, "");
                self.st.get_mut(id).ty = Type::TypeParam(id);
                let inner = self.st.get(*tp).tparams.clone();
                let inner_new = self.fresh_method_tparams(id, &inner);
                self.st.get_mut(id).tparams = inner_new;
                id
            })
            .collect();
        // An F-bound (`C[T <: Ordered[T]]`) names the parameters being copied.
        let to: Vec<Type> = out.iter().map(|t| Type::TypeParam(*t)).collect();
        for (i, tp) in src.iter().enumerate() {
            let hi = self.st.get(*tp).bound_hi.clone();
            let lo = self.st.get(*tp).bound_lo.clone();
            self.st.get_mut(out[i]).bound_hi =
                hi.map(|t| crate::symbol::subst_tparams_slice(src, &to, &t));
            self.st.get_mut(out[i]).bound_lo =
                lo.map(|t| crate::symbol::subst_tparams_slice(src, &to, &t));
        }
        out
    }

    pub(crate) fn type_module(&mut self, tree: &mut Tree) {
        let m = tree.sym;
        let cls = match self.st.get(m).ty {
            Type::ModuleRef(c) => c,
            _ => m,
        };
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        let saved_ret = self.return_meth;
        self.st.owner = cls;
        self.st.this_class = cls;
        self.return_meth = None;
        self.st.push_scope();
        for mem in self.st.get(cls).members.clone() {
            let n = self.st.get(mem).name.clone();
            self.st.enter_in_current(&n, mem);
        }
        let (self_name, self_tpt) = match &tree.kind {
            TreeKind::ModuleDef { impl_, .. } => (impl_.self_name.clone(), impl_.self_tpt.clone()),
            _ => (None, None),
        };
        let mod_span = tree.span;
        let (body, parents) = match &mut tree.kind {
            TreeKind::ModuleDef { impl_, .. } => (&mut impl_.body, &mut impl_.parents),
            _ => return,
        };
        let mut pts = Vec::new();
        let saved_parent_ctx = self.parent_ctx.replace((cls, saved_this));
        for p in parents.iter_mut() {
            // Parents are types: `object B extends B` extends the *trait* B,
            // not itself. Typing them as expressions picks the module.
            self.type_parent(p);
            pts.push(p.ty.clone());
        }
        pts.retain(|t| !matches!(t, Type::ModuleRef(m) if *m == cls));
        self.parent_ctx = saved_parent_ctx;
        if !pts.is_empty() {
            self.st.get_mut(cls).parents = pts;
        }
        self.check_mixin_superclasses(cls, parents, mod_span);
        // A `case object`'s module class is a `Product` too; a case class's
        // companion is not, and `wants_product` tells them apart by the `CASE`
        // flag the namer copies from the `object`'s own modifiers.
        self.link_case_product(cls);
        self.register_sealed_child(cls);
        self.enter_inherited_members(cls);
        self.bind_self_type(cls, self_name, self_tpt.as_deref());
        self.presig_import_prefixes(body);
        let saved_missed = self.import_prefix_missed;
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::Import { .. }) {
                self.type_import(stt);
            }
        }
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_type_member(stt);
            }
        }
        self.finish_type_aliases(body);
        for stt in body.iter_mut() {
            if !matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_member_sig_deferrable(stt);
            }
        }
        self.import_prefix_missed = saved_missed;
        if !self.sigs_only {
            for stt in body.iter_mut() {
                self.type_member_body(stt);
            }
        }
        self.check_self_conformance(cls, tree.span);
        self.check_mixin_parents(cls, tree.span);
        self.check_type_member_kind_override(cls, tree.span);
        if !self.sigs_only {
            let body_snapshot: Vec<Tree> = body.to_vec();
            self.check_abstract_override_placement(cls, &body_snapshot);
            // scalac 2.13.16 reports an `object` the same way it reports a
            // `new`: the instance is what cannot be built.
            let headline = "object creation impossible.".to_string();
            self.check_abstract_override_grounded(cls, mod_span, &headline);
            self.check_overrides(cls, &body_snapshot, mod_span);
            self.check_double_defs(cls, &body_snapshot);
            self.check_missing_implementations(cls, mod_span, &headline);
        }
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        self.return_meth = saved_ret;
        tree.ty = Type::ModuleRef(cls);
    }

    /// Resolve one `type T = rhs` on demand, right-hand side and all, the way
    /// the enclosing template's own pass would have. Used by `complete_lazy_sig`
    /// when a reference from another unit reaches the alias first.
    pub(crate) fn complete_type_alias_tree(&mut self, tree: &mut Tree) {
        self.type_type_member(tree);
        self.finish_one_type_alias(tree);
    }

    pub(crate) fn type_type_member(&mut self, tree: &mut Tree) {
        if self.take_lazy_done(tree) {
            return;
        }
        self.drop_lazy_sig(tree.sym);
        let span = tree.span;
        let type_member_id = tree.sym;
        let (has_tparams, has_bounds, name) = match &tree.kind {
            TreeKind::TypeDef {
                tparams,
                lo,
                hi,
                name,
                ..
            } => (
                !tparams.is_empty(),
                lo.is_some() || hi.is_some(),
                name.clone(),
            ),
            _ => return,
        };
        if has_bounds && !has_tparams {
            let ty = if let TreeKind::TypeDef { rhs, lo, hi, .. } = &mut tree.kind {
                let lo_ty = lo.as_ref().map(|t| self.tree_to_type(t));
                let hi_ty = hi.as_ref().map(|t| self.tree_to_type(t));
                if let Some(t) = &lo_ty {
                    self.check_proper_type(t, span);
                }
                if let Some(t) = &hi_ty {
                    self.check_proper_type(t, span);
                }
                if !type_member_id.is_none() {
                    self.st.get_mut(type_member_id).bound_lo = lo_ty.clone();
                    self.st.get_mut(type_member_id).bound_hi = hi_ty.clone();
                }
                if rhs.is_empty() {
                    Type::TypeMember(type_member_id)
                } else {
                    let rhs_ty = self.tree_to_type(rhs);
                    self.check_proper_type(&rhs_ty, span);
                    self.check_alias_against_bounds(
                        span,
                        &name,
                        &rhs_ty,
                        lo_ty.as_ref(),
                        hi_ty.as_ref(),
                    );
                    rhs_ty
                }
            } else {
                return;
            };
            tree.ty = ty.clone();
            if !type_member_id.is_none() {
                self.st.get_mut(type_member_id).ty = ty;
            }
            return;
        }
        if has_tparams {
            let ty = if let TreeKind::TypeDef {
                tparams,
                rhs,
                lo,
                hi,
                ..
            } = &mut tree.kind
            {
                self.st.push_scope();
                let tp_ids = self.enter_tparams(tparams, type_member_id);
                if !type_member_id.is_none() {
                    self.st.get_mut(type_member_id).tparams = tp_ids;
                }
                let lo_ty = lo.as_ref().map(|t| self.tree_to_type(t));
                let hi_ty = hi.as_ref().map(|t| self.tree_to_type(t));
                if let Some(t) = &lo_ty {
                    self.check_proper_type(t, span);
                }
                if let Some(t) = &hi_ty {
                    self.check_proper_type(t, span);
                }
                if !type_member_id.is_none() {
                    self.st.get_mut(type_member_id).bound_lo = lo_ty;
                    self.st.get_mut(type_member_id).bound_hi = hi_ty.clone();
                }
                let ty = if rhs.is_empty() {
                    Type::TypeMember(type_member_id)
                } else {
                    let rhs_ty = self.tree_to_type(rhs);
                    self.check_proper_type(&rhs_ty, span);
                    if let Some(h) = &hi_ty {
                        if !rhs_ty.is_error() && !self.st.is_sub_type(&rhs_ty, h) {
                            self.error(
                                span,
                                format!(
                                    "incompatible type in overriding: {} does not conform to {}",
                                    self.st.display_type(&rhs_ty),
                                    self.st.display_type(h)
                                ),
                            );
                        }
                    }
                    rhs_ty
                };
                self.st.pop_scope();
                ty
            } else {
                return;
            };
            tree.ty = ty.clone();
            if !type_member_id.is_none() {
                self.st.get_mut(type_member_id).ty = ty;
            }
            let _ = name;
            return;
        }
        let rhs_empty = match &tree.kind {
            TreeKind::TypeDef { rhs, .. } => rhs.is_empty(),
            _ => return,
        };
        let ty = if rhs_empty {
            Type::TypeMember(type_member_id)
        } else if let TreeKind::TypeDef { rhs, .. } = &tree.kind {
            self.tree_to_type(rhs)
        } else {
            return;
        };
        tree.ty = ty.clone();
        if !type_member_id.is_none() {
            self.st.get_mut(type_member_id).ty = ty;
        }
        let _ = name;
    }

    /// nsc: overriding `type F[_]` with `type F` (or the reverse) is a kind mismatch.
    fn check_type_member_kind_override(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        let members = self.st.get(class_id).members.clone();
        let parents = self.st.get(class_id).parents.clone();
        for mid in members {
            if self.st.get(mid).kind != SymKind::TypeMember || self.st.get(mid).owner != class_id {
                continue;
            }
            let name = self.st.get(mid).name.clone();
            let child_arity = self.st.get(mid).tparams.len();
            for p in &parents {
                let Some(pcls) = self.st.class_sym_of(p) else {
                    continue;
                };
                for m in self.st.lookup_member(pcls, &name) {
                    if self.st.get(m).kind != SymKind::TypeMember {
                        continue;
                    }
                    if self.st.get(m).owner == class_id {
                        continue;
                    }
                    let parent_arity = self.st.get(m).tparams.len();
                    if child_arity != parent_arity {
                        self.error(
                            span,
                            format!(
                                "illegal inheritance: type member {name} has incompatible kinds (child takes {child_arity} type parameters, parent takes {parent_arity})"
                            ),
                        );
                    } else {
                        // nsc compares the two declarations after aligning the
                        // parent's type parameters with the child's, so
                        // `type C[T] <: TypedType[T]` overridden by
                        // `type C[T] = JdbcType[T]` compares `JdbcType[T_child]`
                        // against `TypedType[T_child]`, not `TypedType[T_parent]`.
                        let align = |st: &crate::symbol::SymbolTable, t: Option<Type>| {
                            let ptps = st.get(m).tparams.clone();
                            let ctps = st.get(mid).tparams.clone();
                            let t = if ptps.is_empty() || ptps.len() != ctps.len() {
                                t
                            } else {
                                let args: Vec<Type> =
                                    ctps.iter().map(|&c| Type::TypeParam(c)).collect();
                                t.map(|t| crate::symbol::subst_tparams_slice(&ptps, &args, &t))
                            };
                            // The bound is stated in the parent's terms, so a
                            // sibling member it mentions (`type B[T] <: C[T]`)
                            // must be re-read as the child implements it, and
                            // a `this.type` in it (`type Self >: this.type`)
                            // means *the child's* `this` at the override site.
                            let t = t.map(|t| retarget_this(&t, class_id));
                            // The bound also mentions the *enclosing class's*
                            // type parameters, and those are the parent's, not
                            // the child's: `trait Ops[F[_]] { type T <:
                            // Functor[F] }` read at `trait AllOps[F[_]] extends
                            // Ops[F]` has to become `Functor[F_AllOps]` before
                            // it can be compared with the child's own bound.
                            // Without this every re-declaration that narrows a
                            // bound in a *generic* trait was rejected -- cats'
                            // whole `AllOps` layer restates `type TypeClassType
                            // <: Functor[F]` at each level.
                            let t = t.map(|t| st.subst_as_seen_from(&Type::ThisType(class_id), &t));
                            t.map(|t| st.expand_type_members(class_id, &t))
                        };
                        let parent_hi = align(&self.st, self.st.get(m).bound_hi.clone());
                        let parent_lo = align(&self.st, self.st.get(m).bound_lo.clone());
                        let child_ty = self.st.get(mid).ty.clone();
                        let child_hi = self.st.get(mid).bound_hi.clone();
                        let child_lo = self.st.get(mid).bound_lo.clone();
                        let child_abs = matches!(&child_ty, Type::TypeMember(id) if *id == mid);
                        if let Some(phi) = parent_hi {
                            let ok = if child_abs {
                                child_hi
                                    .as_ref()
                                    .map(|h| self.st.is_sub_type(h, &phi))
                                    .unwrap_or(false)
                            } else {
                                self.st.is_sub_type(&child_ty, &phi)
                            };
                            if !ok && !child_ty.is_error() && !phi.is_error() {
                                self.error(
                                    span,
                                    format!(
                                        "incompatible type in overriding type {name}: {} does not conform to <: {}",
                                        self.st.display_type(&child_ty),
                                        self.st.display_type(&phi)
                                    ),
                                );
                            }
                        }
                        if let Some(plo) = parent_lo {
                            let ok = if child_abs {
                                child_lo
                                    .as_ref()
                                    .map(|l| self.st.is_sub_type(&plo, l))
                                    .unwrap_or(false)
                            } else {
                                self.st.is_sub_type(&plo, &child_ty)
                            };
                            if !ok && !child_ty.is_error() && !plo.is_error() {
                                self.error(
                                    span,
                                    format!(
                                        "incompatible type in overriding type {name}: {} does not conform to >: {}",
                                        self.st.display_type(&child_ty),
                                        self.st.display_type(&plo)
                                    ),
                                );
                            }
                        }
                    }
                    break;
                }
            }
        }
    }

    fn check_alias_against_bounds(
        &mut self,
        span: Span,
        name: &str,
        rhs: &Type,
        lo: Option<&Type>,
        hi: Option<&Type>,
    ) {
        if rhs.is_error() {
            return;
        }
        if let Some(h) = hi {
            if !h.is_error() && !self.st.is_sub_type(rhs, h) {
                self.error(
                    span,
                    format!(
                        "incompatible type in overriding type {name}: {} does not conform to <: {}",
                        self.st.display_type(rhs),
                        self.st.display_type(h)
                    ),
                );
            }
        }
        if let Some(l) = lo {
            if !l.is_error() && !self.st.is_sub_type(l, rhs) {
                self.error(
                    span,
                    format!(
                        "incompatible type in overriding type {name}: {} does not conform to >: {}",
                        self.st.display_type(rhs),
                        self.st.display_type(l)
                    ),
                );
            }
        }
    }

    /// Expand `type T = U` chains and diagnose `illegal cyclic reference`.
    /// Abstract `type A` (empty rhs) is left as `TypeMember`.
    /// `finish_type_aliases` for a single definition, for the on-demand path.
    pub(crate) fn finish_one_type_alias(&mut self, stt: &mut Tree) {
        let mut alias_ids = HashSet::new();
        if let TreeKind::TypeDef { rhs, .. } = &stt.kind {
            if !rhs.is_empty() && !stt.sym.is_none() {
                alias_ids.insert(stt.sym.0);
            }
        }
        if alias_ids.is_empty() {
            return;
        }
        self.expand_one_alias(stt, &alias_ids);
    }

    pub(crate) fn finish_type_aliases(&mut self, body: &mut [Tree]) {
        let mut alias_ids = HashSet::new();
        for stt in body.iter() {
            if let TreeKind::TypeDef { rhs, .. } = &stt.kind {
                if !rhs.is_empty() && !stt.sym.is_none() {
                    alias_ids.insert(stt.sym.0);
                }
            }
        }
        for stt in body.iter_mut() {
            self.expand_one_alias(stt, &alias_ids);
        }
    }

    fn expand_one_alias(&mut self, stt: &mut Tree, alias_ids: &HashSet<u32>) {
        let TreeKind::TypeDef { name, rhs, .. } = &stt.kind else {
            return;
        };
        if rhs.is_empty() || stt.sym.is_none() || stt.ty.is_error() {
            return;
        }
        let name = name.clone();
        let span = stt.span;
        let mut seen = Vec::new();
        match expand_alias_type(&self.st, &stt.ty, alias_ids, &mut seen) {
            Ok(t) => {
                stt.ty = t.clone();
                self.st.get_mut(stt.sym).ty = t;
            }
            Err(_) => {
                self.error(
                    span,
                    format!("illegal cyclic reference involving type {name}"),
                );
                stt.ty = Type::Error;
                self.st.get_mut(stt.sym).ty = Type::Error;
            }
        }
    }

    fn bind_self_type(
        &mut self,
        class_id: SymbolId,
        self_name: Option<String>,
        self_tpt: Option<&Tree>,
    ) {
        let st = match self_tpt {
            Some(tpt) => {
                // A self type names its parts the same way an `extends` clause
                // does; `trait N { self: Missing => }` is `not found: type
                // Missing`, not an "illegal inheritance" against a placeholder.
                //
                // The signature pass's complaint is dropped, exactly as
                // `type_parent_ctor_app`'s is: a class header is typed by both
                // passes, so anything real is raised again by the pass that
                // runs with every unit's signatures in hand. What the
                // signature pass alone cannot see is a self type named through
                // an import whose prefix is another template's `val`:
                // gitbucket's `trait TemplateComponent { self: Profile =>
                // import profile.api._; trait BasicTemplate { self: Table[?] =>
                // … } }` had `Table` in scope on the body pass and not on the
                // signature pass, and `not found: type Table` -- with the whole
                // template's members missing behind it -- was permanent.
                let mark = self.diags.len();
                let t = self.with_strict_type_names(|s| s.tree_to_type(tpt));
                if self.sigs_only {
                    self.diags.truncate(mark);
                }
                if t.is_error() {
                    return;
                }
                self.st.get_mut(class_id).self_type = Some(t.clone());
                t
            }
            // `trait T { self => … }` names `this` without narrowing it.
            None => match &self_name {
                Some(_) => Type::ThisType(class_id),
                None => return,
            },
        };
        // A *compound* self type offers the members of every part:
        // `self: ControllerBase with AccountService with RepositoryService =>`
        // (and, under -Xsource:3, the `&` spelling) is how gitbucket writes
        // every cake trait. `class_sym_of` answers for one class, so taking
        // only that left the other parts' members out of scope entirely --
        // 230 "not found: value ownerOnly / referrersOnly / …".
        let roots: Vec<Type> = match &st {
            Type::Refined { parents, .. } => parents.clone(),
            other => vec![other.clone()],
        };
        let mut seen = std::collections::HashSet::new();
        for root in roots {
            let Some(cls) = self.st.class_sym_of(&root) else {
                continue;
            };
            if !seen.insert(cls.0) {
                continue;
            }
            self.enter_members_of(cls);
            // members of Foo's parents too (lookup_member walks them; Ident needs scope)
            let mut work = self.st.get(cls).parents.clone();
            while let Some(p) = work.pop() {
                let Some(pid) = self.st.class_sym_of(&p) else {
                    continue;
                };
                if !seen.insert(pid.0) {
                    continue;
                }
                self.enter_members_of(pid);
                work.extend(self.st.get(pid).parents.clone());
            }
        }
        if let Some(name) = self_name {
            if name != "this" {
                // The signature pass and the body pass both bind the alias;
                // allocating twice would make `self` look like an overload.
                let sid = self.st.get(class_id).self_alias.unwrap_or_else(|| {
                    self.st
                        .alloc(&name, class_id, SymKind::Term, Flags::SYNTHETIC, "")
                });
                self.st.get_mut(class_id).self_alias = Some(sid);
                self.st.get_mut(sid).ty = st;
                self.st.enter_in_current(&name, sid);
            }
        }
    }

    /// Bring `cls`'s members into the current scope, minus the ones another
    /// template never inherits: its constructor, its compiler-made `$` names
    /// and its self alias (see `Symbol::self_alias`).
    fn enter_members_of(&mut self, cls: SymbolId) {
        let alias = self.st.get(cls).self_alias;
        for m in self.st.get(cls).members.clone() {
            if Some(m) == alias {
                continue;
            }
            let n = self.st.get(m).name.clone();
            if n.ends_with('$') || n == "<init>" {
                continue;
            }
            self.st.enter_in_current(&n, m);
        }
    }

    /// Put inherited members in the template scope so `val Red = Value` inside
    /// `object Color extends Enumeration` resolves `Value` like nsc.
    pub(crate) fn enter_inherited_members(&mut self, cls: SymbolId) {
        // Breadth-first, last parent first: that is the order a member reaches
        // the class through nsc's linearization, and the scope keeps the first
        // entry for a name. Walking depth-first let a *grandparent's* deferred
        // declaration be entered before its own subclass's concrete one, so
        // `new SimpleFeatureNode[T] with SimpleFunction` saw `Node`'s abstract
        // `type Self` instead of `SimpleFeatureNode`'s `type Self = …`.
        let mut work: std::collections::VecDeque<Type> =
            self.st.get(cls).parents.iter().rev().cloned().collect();
        let mut seen = std::collections::HashSet::new();
        seen.insert(cls.0);
        while let Some(p) = work.pop_front() {
            let Some(pid) = self.st.class_sym_of(&p) else {
                continue;
            };
            if !seen.insert(pid.0) {
                continue;
            }
            let alias = self.st.get(pid).self_alias;
            for m in self.st.get(pid).members.clone() {
                let n = self.st.get(m).name.clone();
                if n.ends_with('$') || n == "<init>" {
                    continue;
                }
                // A parent's type parameters are not inherited names; entering
                // them shadows an enclosing `A` of the same name
                // (`def wrap[A](…) = new Show[A] { … }`).
                if self.st.get(m).kind == SymKind::TypeParam {
                    continue;
                }
                // Neither is a parent's self alias (see `Symbol::self_alias`).
                if Some(m) == alias {
                    continue;
                }
                // A `private` member is not inherited at all (SLS 5.2): a bare
                // constructor parameter is one, so `class B(st: Int) extends
                // A(…)` where `A` also takes an `st` sees only its own.
                // Plain `private` counts too -- `trait A { private val x = 1 }`
                // mixed in beside `trait B { val x = 2 }` used to answer `x`
                // with A's, purely because the traversal reached A first
                // (`run/t7475b`). A qualified `private[C]` stays visible: the
                // qualifier can name a package that encloses the subclass.
                let s = self.st.get(m);
                if s.flags.contains(Flags::PRIVATE) && s.private_within.is_none() {
                    continue;
                }
                self.st.enter_in_current(&n, m);
            }
            work.extend(self.st.get(pid).parents.iter().rev().cloned());
        }
    }

    /// SLS 5.3.3: `T` may only be mixed into a subclass of `T`'s own
    /// superclass. `parents` supplies the span so the caret lands on the
    /// offending mixin, as scalac's does.
    fn check_mixin_superclasses(&mut self, class_id: SymbolId, parents: &[Tree], span: Span) {
        for e in crate::traitparent::check_mixin_superclasses(&self.st, class_id) {
            let at = parents.get(e.parent_index).map(|p| p.span).unwrap_or(span);
            self.error(at, e.message);
        }
    }

    /// `abstract override` is only meaningful where `super` is linearized,
    /// which is only in a trait. scalac 2.13.16 points at the member.
    fn check_abstract_override_placement(&mut self, class_id: SymbolId, body: &[Tree]) {
        if class_id.is_none() || self.st.get(class_id).flags.contains(Flags::TRAIT) {
            return;
        }
        for stt in body {
            let TreeKind::DefDef { mods, .. } = &stt.kind else {
                continue;
            };
            if mods.flags.contains(Flags::ABSTRACT) && mods.flags.contains(Flags::OVERRIDE) {
                self.error(
                    stt.span,
                    "`abstract override` modifier only allowed for members of traits".to_string(),
                );
            }
        }
    }

    /// Every `abstract override` a *concrete* class inherits must reach a real
    /// implementation further down the linearization; otherwise `super.m`
    /// inside the trait has no target. Without this the backend emitted a
    /// `throw new RuntimeException("no super implementation for …")` stub.
    fn check_abstract_override_grounded(&mut self, class_id: SymbolId, span: Span, headline: &str) {
        if class_id.is_none() {
            return;
        }
        let s = self.st.get(class_id);
        if s.flags.contains(Flags::TRAIT)
            || s.flags.contains(Flags::INTERFACE)
            || s.flags.contains(Flags::ABSTRACT)
        {
            return;
        }
        for msg in
            crate::traitparent::check_abstract_override_grounded(&self.st, class_id, headline)
        {
            self.error(span, msg);
        }
    }

    /// SLS 5.1.4: every rule an override has to satisfy. scalac points at the
    /// offending *member*, so the body snapshot supplies its span; a member
    /// with no tree of its own (a constructor field) falls back to the
    /// template's.
    fn check_overrides(&mut self, class_id: SymbolId, body: &[Tree], span: Span) {
        // Which members wrote their result type. nsc types the others *at* the
        // overridden member's result type, so they conform by construction;
        // see `override_check::check_pair`.
        let mut inferred: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for t in body {
            let tpt = match &t.kind {
                TreeKind::DefDef { tpt, .. } | TreeKind::ValDef { tpt, .. } => tpt,
                _ => continue,
            };
            if tpt.is_empty() && !t.sym.is_none() {
                inferred.insert(t.sym.0);
            }
        }
        let is_inferred = |s: SymbolId| inferred.contains(&s.0);
        for e in crate::override_check::check_overrides(&self.st, class_id, &is_inferred) {
            let at = body
                .iter()
                .find(|t| t.sym == e.sym && !e.sym.is_none())
                .map(|t| t.span)
                .unwrap_or(span);
            self.error(at, e.message);
        }
    }

    /// nsc's `RefChecks.checkNoDoubleDefs`: two overloads of one template that
    /// erase to the same descriptor. See `crate::double_def`.
    fn check_double_defs(&mut self, class_id: SymbolId, body: &[Tree]) {
        if class_id.is_none() {
            return;
        }
        let members: Vec<SymbolId> = body
            .iter()
            .filter(|t| matches!(t.kind, TreeKind::DefDef { .. }))
            .map(|t| t.sym)
            .collect();
        for e in crate::double_def::check_double_defs(&self.st, class_id, &members) {
            let Some(at) = body
                .iter()
                .find(|t| t.sym == e.sym && !e.sym.is_none())
                .map(|t| t.span)
            else {
                continue;
            };
            self.error(at, e.message);
        }
    }

    /// SLS 5.2.6: a concrete template must implement every deferred member it
    /// inherits. Without this a missing implementation compiled and then threw
    /// `AbstractMethodError` at the first call.
    fn check_missing_implementations(&mut self, class_id: SymbolId, span: Span, headline: &str) {
        if class_id.is_none() {
            return;
        }
        let s = self.st.get(class_id);
        if s.flags.contains(Flags::TRAIT)
            || s.flags.contains(Flags::INTERFACE)
            || s.flags.contains(Flags::ABSTRACT)
        {
            return;
        }
        if let Some(msg) =
            crate::override_check::check_missing_implementations(&self.st, class_id, headline)
        {
            self.error(span, msg);
        }
    }

    /// nsc's `validateParentClasses`: only the *first* parent of a template may
    /// be a class. Everything mixed in after it has to be a trait -- a second
    /// class would bring a second constructor with it.
    ///
    /// This is a rule about templates, not about types: `def f(x: A with B)` is
    /// a legal signature for two unrelated classes (the type is simply
    /// uninhabited), and rejecting it turned slick's
    /// `Query[B, BU, C] & TableQuery[B]` -- where one *is* a subclass of the
    /// other -- into an error scalac does not report.
    fn check_mixin_parents(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        for p in self.st.get(class_id).parents.clone().iter().skip(1) {
            let Some(ps) = self.st.class_sym_of(p) else {
                continue;
            };
            let f = self.st.get(ps).flags;
            if f.contains(Flags::TRAIT) || f.contains(Flags::INTERFACE) {
                continue;
            }
            let name = self.st.get(ps).name.clone();
            self.error(
                span,
                format!("class {name} needs to be a trait to be mixed in"),
            );
        }
    }

    fn check_self_conformance(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        let is_trait = self.st.get(class_id).flags.contains(Flags::TRAIT);
        if is_trait {
            return;
        }
        // `class C[F[_]] extends P[F]` conforms to `P`'s self type as `C[F]`,
        // not as a bare `C`: dropping the arguments made every parameterized
        // cake class "not conform".
        let this_ty = self.st.self_type_of_class(class_id);
        let mut work = vec![class_id];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            if let Some(st) = self.st.get(id).self_type.clone() {
                // The self type was written in the *declaring* trait's
                // vocabulary. Two things separate it from what it means here:
                // the parent's type parameters (`this: Database[F] =>` on
                // `BasicDatabaseDef[F]`), and the abstract type members its
                // enclosing cake left open (`type Database[F[_]]` on
                // `BasicBackend`, aliased to `JdbcDatabaseDef[F]` by
                // `JdbcBackend`). Reading it raw compares `JdbcDatabaseDef`
                // against `BasicBackend.Database[F]`, which nothing satisfies.
                let st = self.st.subst_as_seen_from(&this_ty, &st);
                let st = self.st.expand_type_members(class_id, &st);
                if !self.st.is_sub_type(&this_ty, &st) {
                    self.error(
                        span,
                        format!(
                            "illegal inheritance: self-type {} does not conform to {}",
                            self.st.display_type(&this_ty),
                            self.st.display_type(&st)
                        ),
                    );
                }
            }
            for p in self.st.get(id).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                }
            }
        }
    }

    fn check_class_variance(&mut self, class_id: SymbolId, span: Span) {
        let tps = self.st.get(class_id).tparams.clone();
        if tps.is_empty() {
            return;
        }
        let mut vars: Vec<(SymbolId, i8, String)> = Vec::new();
        for tp in &tps {
            let f = self.st.get(*tp).flags;
            let v = if f.contains(Flags::COVARIANT) {
                1
            } else if f.contains(Flags::CONTRAVARIANT) {
                -1
            } else {
                0
            };
            vars.push((*tp, v, self.st.get(*tp).name.clone()));
        }
        if vars.iter().all(|(_, v, _)| *v == 0) {
            return;
        }
        let class_is_case = self.st.get(class_id).flags.contains(Flags::CASE);
        for f in self.st.get(class_id).ctor_fields.clone() {
            let flags = self.st.get(f).flags;
            let ty = self.st.get(f).ty.clone();
            let name = self.st.get(f).name.clone();
            if flags.contains(Flags::MUTABLE) {
                self.check_variance_ty(&vars, &ty, -1, span, &format!("value {name}"));
                self.check_variance_ty(&vars, &ty, 1, span, &format!("value {name}"));
            } else if flags.contains(Flags::ACCESSOR) || class_is_case {
                self.check_variance_ty(&vars, &ty, 1, span, &format!("value {name}"));
            }
        }
        for m in self.st.get(class_id).members.clone() {
            if self.st.get(m).kind != SymKind::Method {
                continue;
            }
            // nsc does not variance-check what it synthesized: `case class
            // Box[+A](a: A)` is legal even though its `copy(a: A)` puts `A`
            // in a contravariant position.
            if self.st.get(m).flags.contains(Flags::SYNTHETIC) {
                continue;
            }
            let name = self.st.get(m).name.clone();
            if name == "<init>" || name == "<clinit>" {
                continue;
            }
            if let Type::Method { paramss, ret } = self.st.get(m).ty.clone() {
                for (i, p) in paramss.iter().flatten().enumerate() {
                    self.check_variance_ty(&vars, p, -1, span, &format!("parameter {i} of {name}"));
                }
                self.check_variance_ty(&vars, &ret, 1, span, &format!("return type of {name}"));
            }
        }
    }

    /// Declared variances of `sym`'s own type parameters (`+` → 1, `-` → -1).
    /// Works for classes, abstract type members and higher-kinded type
    /// parameters alike; they all hang their parameters off `tparams`.
    fn tparam_variances(&self, sym: SymbolId) -> Vec<i8> {
        self.st
            .get(sym)
            .tparams
            .iter()
            .map(|tp| {
                let f = self.st.get(*tp).flags;
                if f.contains(Flags::COVARIANT) {
                    1
                } else if f.contains(Flags::CONTRAVARIANT) {
                    -1
                } else {
                    0
                }
            })
            .collect()
    }

    fn check_variance_ty(
        &mut self,
        vars: &[(SymbolId, i8, String)],
        ty: &Type,
        pos: i8,
        span: Span,
        where_: &str,
    ) {
        match ty {
            Type::TypeParam(id) => {
                if let Some((_, vp, name)) = vars.iter().find(|(t, _, _)| t == id) {
                    if *vp != 0 && pos != 0 && (*vp < 0 && pos > 0 || *vp > 0 && pos < 0) {
                        let which = if *vp > 0 {
                            "covariant"
                        } else {
                            "contravariant"
                        };
                        let place = if pos > 0 {
                            "covariant"
                        } else {
                            "contravariant"
                        };
                        self.error(
                            span,
                            format!(
                                "{which} type {name} occurs in {place} position in type {} of {where_}",
                                self.st.display_type(ty)
                            ),
                        );
                    }
                    if *vp != 0 && pos == 0 {
                        let which = if *vp > 0 {
                            "covariant"
                        } else {
                            "contravariant"
                        };
                        self.error(
                            span,
                            format!(
                                "{which} type {name} occurs in invariant position in type {} of {where_}",
                                self.st.display_type(ty)
                            ),
                        );
                    }
                }
            }
            Type::Class { sym, args } => {
                let vs = self.tparam_variances(*sym);
                for (i, a) in args.iter().enumerate() {
                    let vp = vs.get(i).copied().unwrap_or(0);
                    self.check_variance_ty(vars, a, pos * vp, span, where_);
                }
            }
            Type::Applied { ctor, args } => {
                self.check_variance_ty(vars, ctor, pos, span, where_);
                // nsc reads the variances off `sym.typeParams` of whatever the
                // application heads on, not off classes alone. An abstract type
                // member (`type M[+X] <: ...`) and a higher-kinded type
                // parameter (`F[+X]`) both carry declared variances, and both
                // must be honoured -- treating their arguments as invariant
                // rejects `def head: ResultAction[T, NoStream, E]`, which nsc
                // accepts.
                let vs = match ctor.as_ref() {
                    Type::TypeMember(id) | Type::TypeParam(id) => self.tparam_variances(*id),
                    Type::Class { sym, args: pre } => {
                        let mut vs = self.tparam_variances(*sym);
                        vs.drain(0..pre.len().min(vs.len()));
                        vs
                    }
                    _ => Vec::new(),
                };
                for (i, a) in args.iter().enumerate() {
                    let vp = vs.get(i).copied().unwrap_or(0);
                    self.check_variance_ty(vars, a, pos * vp, span, where_);
                }
            }
            Type::Function { params, ret } => {
                for p in params {
                    self.check_variance_ty(vars, p, -pos, span, where_);
                }
                self.check_variance_ty(vars, ret, pos, span, where_);
            }
            Type::Method { paramss, ret } => {
                for p in paramss.iter().flatten() {
                    self.check_variance_ty(vars, p, -pos, span, where_);
                }
                self.check_variance_ty(vars, ret, pos, span, where_);
            }
            // `Array` is invariant; `=> T` and `T*` keep the position.
            Type::Array(t) => {
                self.check_variance_ty(vars, t, 0, span, where_);
            }
            Type::ByName(t) | Type::Repeated(t) => {
                self.check_variance_ty(vars, t, pos, span, where_);
            }
            Type::Tuple(ts) => {
                for t in ts {
                    self.check_variance_ty(vars, t, pos, span, where_);
                }
            }
            Type::Named { args, .. } => {
                for a in args {
                    self.check_variance_ty(vars, a, 0, span, where_);
                }
            }
            Type::Refined { parents, decls } => {
                for p in parents {
                    self.check_variance_ty(vars, p, pos, span, where_);
                }
                for d in decls {
                    match d {
                        scala_rs_parser::RefineDecl::Type { rhs: Some(t), .. }
                        | scala_rs_parser::RefineDecl::Val { ty: t, .. } => {
                            self.check_variance_ty(vars, t, pos, span, where_);
                        }
                        scala_rs_parser::RefineDecl::Def { paramss, ret, .. } => {
                            for p in paramss.iter().flatten() {
                                self.check_variance_ty(vars, p, -pos, span, where_);
                            }
                            self.check_variance_ty(vars, ret, pos, span, where_);
                        }
                        _ => {}
                    }
                }
            }
            Type::Annotated { tpe, annot } => {
                // nsc skips only `@uncheckedVariance`, not `@unchecked`.
                if annot.rsplit('.').next().unwrap_or(annot.as_str()) != "uncheckedVariance" {
                    self.check_variance_ty(vars, tpe, pos, span, where_);
                }
            }
            _ => {}
        }
    }

    /// `x.f = v` where `f` resolved to a getter and the receiver has an
    /// `f_=`: the assignment is that call, not a field store.
    pub(crate) fn setter_assign_lhs(&mut self, lhs: &Tree) -> bool {
        let TreeKind::Select { qual, name } = &lhs.kind else {
            return false;
        };
        if lhs.sym.is_none() || self.st.get(lhs.sym).kind != SymKind::Method {
            return false;
        }
        let Some(cls) = self.st.class_sym_of(&qual.ty) else {
            return false;
        };
        let setter = format!("{name}_=");
        self.st
            .lookup_member(cls, &setter)
            .into_iter()
            .any(|m| self.st.get(m).kind == SymKind::Method)
    }

    /// nsc's `reassignment to val`. Without it `d.v = 5` on a trait's `val`
    /// type-checks and then fails at run time: the mixin setter a trait `val`
    /// gets is not a setter a program may call.
    pub(crate) fn check_reassignment(&mut self, lhs: &Tree) {
        let id = lhs.sym;
        if id.is_none() {
            return;
        }
        let s = self.st.get(id);
        // Only a term (a field or local) is an l-value here; a `Method` lhs is
        // an already-resolved `x_=` setter or an unrelated resolution failure.
        if s.kind != SymKind::Term || s.flags.contains(Flags::MUTABLE) {
            return;
        }
        // Java fields carry no Scala mutability, and the compiler's own
        // synthetic terms (`$outer`, capture fields, …) are written by the
        // phases that create them.
        if s.flags.contains(Flags::JAVA) || s.flags.contains(Flags::SYNTHETIC) {
            return;
        }
        let name = s.name.clone();
        self.error(lhs.span, format!("reassignment to val {name}"));
    }

    pub(crate) fn check_stored_annotations(&mut self, tree: &Tree) {
        let mods = match &tree.kind {
            TreeKind::DefDef { mods, .. }
            | TreeKind::ValDef { mods, .. }
            | TreeKind::ClassDef { mods, .. }
            | TreeKind::ModuleDef { mods, .. }
            | TreeKind::TypeDef { mods, .. } => mods,
            _ => return,
        };
        for a in &mods.annotations {
            let path = a.annotation_path();
            if is_tailrec_annot(&path) {
                if !matches!(&tree.kind, TreeKind::DefDef { .. }) {
                    self.error(
                        tree.span,
                        "could not optimize @tailrec annotated method: not a method",
                    );
                } else {
                    self.check_tailrec(tree);
                }
            }
            if is_override_annot(&path) {
                if !matches!(&tree.kind, TreeKind::DefDef { .. }) {
                    self.error(
                        tree.span,
                        format!(
                            "method {} overrides nothing",
                            tree.name().unwrap_or("<annot>")
                        ),
                    );
                } else {
                    self.check_java_override(tree);
                }
            }
            // Real scalac accepts `@inline`/`@noinline` on any definition (val, var,
            // class, type, ...): they are hints consumed only by the bytecode-level
            // optimizer (`-opt:...`), which scala-rs does not implement, and placement
            // is never validated by the typer. See sgap fixtures for confirmation
            // against scalac 2.13.16 (no error, no warning, even with -Xlint:_).
            if is_native_annot(&path) {
                if !matches!(&tree.kind, TreeKind::DefDef { .. }) {
                    self.error(tree.span, "@native is only supported on methods");
                } else if let TreeKind::DefDef { rhs, .. } = &tree.kind {
                    if !rhs.is_empty() {
                        self.error(tree.span, "native method cannot have a body");
                    }
                }
            }
        }
    }

    /// The return type of the member `owner`'s ancestors declare under
    /// `name` with the same value-parameter arity/types, if any of them has
    /// one already known. Used only to seed an unannotated override's own
    /// result type (`type_def_sig`) before its body is typed -- so a
    /// still-`Type::NoType` overridden signature (itself mid-completion) is
    /// skipped rather than borrowed, same as finding nothing.
    pub(crate) fn overridden_ret_type(
        &self,
        owner: SymbolId,
        name: &str,
        my_ps: &[Type],
    ) -> Option<Type> {
        if owner.is_none() || name.is_empty() {
            return None;
        }
        // The candidate may be declared on a *generic* ancestor (`trait
        // Base[A] { def f: A }`), so its raw signature has to be read
        // as-seen-from `owner`'s own type -- the same substitution
        // `bind_found`/`type_select` apply to every other inherited member
        // -- or a type parameter that happens to share a letter with one in
        // scope here reads as itself unsubstituted. Returning the raw type
        // regressed real generic overrides across slick with `type
        // mismatch; found: T required: T`-shaped errors once measured
        // end-to-end, even though the monomorphic case this was written
        // against kept passing on its own.
        let owner_ty = Type::Class {
            sym: owner,
            args: self
                .st
                .get(owner)
                .tparams
                .iter()
                .map(|tp| Type::TypeParam(*tp))
                .collect(),
        };
        let mut seen = std::collections::HashSet::new();
        let mut work: Vec<SymbolId> = self
            .st
            .get(owner)
            .parents
            .clone()
            .iter()
            .filter_map(|p| self.st.class_sym_of(p))
            .collect();
        work.push(self.st.anyref_sym);
        work.push(self.st.any_sym);
        while let Some(id) = work.pop() {
            if id.is_none() || id == owner || !seen.insert(id.0) {
                continue;
            }
            for m in self.st.get(id).members.clone() {
                let (cand_name, cand_kind) = {
                    let cand = self.st.get(m);
                    (cand.name.clone(), cand.kind)
                };
                if cand_name != name || !matches!(cand_kind, SymKind::Method | SymKind::Term) {
                    continue;
                }
                // Deliberately *not* `complete_lazy_sig`: forcing a
                // still-pending candidate to complete here ran it (and
                // whatever forward references its own body makes) before
                // its declaring file's own top-down pass had registered its
                // real scope, so a name only visible via that file's own
                // imports resolved against the bare "owner chain" fallback
                // instead and came back "not found". Measured against
                // slick's `computeCapabilities` chain (`JdbcProfile`
                // overriding `SqlProfile` overriding `RelationalProfile`
                // overriding `BasicProfile`, spread across files ordered
                // alphabetically *after* some of the profile traits that
                // reference them): forcing eager completion here turned 155
                // slick errors into 307. A still-pending candidate's `.ty`
                // is simply skipped, exactly like not finding it -- the
                // walk continues up through this candidate's own further
                // ancestors instead (already queued below), and every real
                // motivating case (`Dumpable.getDumpInfo: DumpInfo`,
                // `QueryInterpreter.run(n: Node): Any`,
                // `BasicProfile.computeCapabilities: Set[Capability]`)
                // bottoms out at an ancestor whose return type was written
                // explicitly and is therefore already known without forcing
                // anything.
                let cand_ty = self.st.get(m).ty.clone();
                let cand_ty = self.st.subst_as_seen_from(&owner_ty, &cand_ty);
                let ps = method_value_params(&cand_ty);
                if ps.len() != my_ps.len() {
                    continue;
                }
                let ok = my_ps
                    .iter()
                    .zip(ps.iter())
                    .all(|(a, b)| a == b || self.st.is_sub_type(a, b) || self.st.is_sub_type(b, a));
                if !ok {
                    continue;
                }
                if let Type::Method { ret, .. } = &cand_ty {
                    if !ret.is_no_type() {
                        return Some(self.own_type_members(owner, ret));
                    }
                }
            }
            for p in self.st.get(id).parents.clone() {
                if let Some(c) = self.st.class_sym_of(&p) {
                    work.push(c);
                }
            }
        }
        None
    }

    /// An inherited signature's abstract type members, read through the class
    /// that inherits it. `trait Node { type Self <: Node; def rebuild(…): Self }`
    /// overridden in `case class StructNode(…) { type Self = StructNode }` has
    /// result type `StructNode` -- nsc sees the declaration as-seen-from
    /// `StructNode.this.type`, so returning a `StructNode` from a `rebuild`
    /// with no written result type is not `found: StructNode required:
    /// Node.Self` (slick's `ast/Node.scala`).
    fn own_type_members(&self, owner: SymbolId, ty: &Type) -> Type {
        let mut members = Vec::new();
        crate::symbol::collect_type_members(ty, &mut members);
        let mut out = ty.clone();
        for m in members {
            let info = self.st.get(m);
            if !info.tparams.is_empty() {
                continue;
            }
            let name = info.name.clone();
            let Some(found) = self
                .st
                .lookup_member(owner, &name)
                .into_iter()
                .find(|&s| s != m && self.st.get(s).kind == SymKind::TypeMember)
            else {
                continue;
            };
            let seen = self.st.dealias(&Type::TypeMember(found));
            if seen.is_no_type()
                || seen.is_error()
                || matches!(&seen, Type::TypeMember(x) if *x == m)
            {
                continue;
            }
            out = crate::symbol::subst_type_member(&out, m, &seen);
        }
        out
    }

    fn check_java_override(&mut self, tree: &Tree) {
        let name = tree.name().unwrap_or("").to_string();
        if name.is_empty() || name == "<init>" {
            self.error(tree.span, "method <init> overrides nothing");
            return;
        }
        if tree.sym.is_none() || !self.method_overrides_parent(tree.sym) {
            self.error(tree.span, format!("method {name} overrides nothing"));
        }
    }

    fn method_overrides_parent(&self, meth: SymbolId) -> bool {
        if meth.is_none() {
            return false;
        }
        let s = self.st.get(meth);
        let name = s.name.clone();
        let owner = s.owner;
        if owner.is_none() {
            return false;
        }
        let my_ps = method_value_params(&s.ty);
        let mut seen = std::collections::HashSet::new();
        let mut work: Vec<SymbolId> = Vec::new();
        for p in &self.st.get(owner).parents {
            if let Some(c) = self.st.class_sym_of(p) {
                work.push(c);
            }
        }
        work.push(self.st.anyref_sym);
        work.push(self.st.any_sym);
        while let Some(id) = work.pop() {
            if id.is_none() || id == owner || !seen.insert(id.0) {
                continue;
            }
            for m in self.st.get(id).members.clone() {
                if m == meth {
                    continue;
                }
                let cand = self.st.get(m);
                if cand.name != name {
                    continue;
                }
                if !matches!(cand.kind, SymKind::Method | SymKind::Term) {
                    continue;
                }
                let ps = method_value_params(&cand.ty);
                if ps.len() != my_ps.len() {
                    continue;
                }
                let ok = my_ps
                    .iter()
                    .zip(ps.iter())
                    .all(|(a, b)| a == b || self.st.is_sub_type(a, b) || self.st.is_sub_type(b, a));
                if ok {
                    return true;
                }
            }
            for p in self.st.get(id).parents.clone() {
                if let Some(c) = self.st.class_sym_of(&p) {
                    work.push(c);
                }
            }
        }
        false
    }

    fn check_tailrec(&mut self, tree: &Tree) {
        let rhs = match &tree.kind {
            TreeKind::DefDef { rhs, .. } => rhs.as_ref(),
            _ => return,
        };
        if !self.tailrec_effectively_final(tree) {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it is neither private nor final so can be overridden",
            );
            return;
        }
        if rhs.is_empty() {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it contains no recursive calls",
            );
            return;
        }
        let mut tail = 0;
        let mut nontail = 0;
        // A call to a *parameterless* method is a bare `Select` -- there is no
        // `Apply` node to recognise. `NominalType.sourceNominalType`
        // (slick's `ast/Type.scala`) is `structuralView match { case n:
        // NominalType => n.sourceNominalType; case _ => this }`, which nsc
        // accepts and this counted as no recursive call at all.
        let nullary = self.st.get(tree.sym).paramss.is_empty();
        count_tailrec_calls(rhs, tree.sym, nullary, true, &mut tail, &mut nontail);
        if nontail > 0 {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it contains a recursive call not in tail position",
            );
        } else if tail == 0 {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it contains no recursive calls",
            );
        }
    }

    fn tailrec_effectively_final(&self, tree: &Tree) -> bool {
        let meth = tree.sym;
        if meth.is_none() {
            return true;
        }
        // A `def` that is a statement of a block is not a member of anything,
        // so nothing can override it. The symbol's owner does not say so: a
        // local def in a `val`'s right-hand side is owned by the enclosing
        // class, because no accessor symbol exists to own it. cats writes ten
        // of these -- `lazy val resolved = { @tailrec def loop… }` inside
        // `case class Deferred` in `instances/{eq,order,show,…}.scala`.
        if self.block_local_defs.contains(&(self.file_index, tree.id)) {
            return true;
        }
        let s = self.st.get(meth);
        if s.flags.contains(Flags::FINAL)
            || s.flags.contains(Flags::PRIVATE)
            || s.flags.contains(Flags::LOCAL)
        {
            return true;
        }
        let owner = s.owner;
        if owner.is_none() {
            return true;
        }
        let o = self.st.get(owner);
        // A def nested in a method or in a value's right-hand side is not a
        // member of anything and cannot be overridden, whatever its owner's
        // own flags say. cats writes `@tailrec def loop` inside `tailRecM`
        // 79 times; nsc accepts every one.
        if matches!(
            o.kind,
            SymKind::Module | SymKind::ModuleClass | SymKind::Method | SymKind::Term
        ) || o.flags.contains(Flags::MODULE)
            || o.flags.contains(Flags::FINAL)
        {
            return true;
        }
        // nsc gives the `$anon` class of a `new C { … }` the FINAL flag, so
        // `Symbol.isEffectivelyFinal` holds for it and every concrete member
        // it declares is eligible. cats writes 7 of these -- among them the
        // instance handed out by `implicit val catsStdInstancesForList:
        // Traverse[List] = new Traverse[List] { @tailrec override def get… }`.
        if o.name.starts_with("$anon") {
            return true;
        }
        // nsc's `isEffectivelyFinalOrNotOverridden`: a *concrete* member of an
        // owner whose subclasses are all known, and none of which overrides
        // it, cannot be overridden either. That covers `sealed class` and --
        // because a class declared in a block can only be extended inside that
        // block -- a method-local class. Both are accepted by scalac 2.13.16
        // and both were rejected here; `tt_tailrec_bad.scala` pins the five
        // shapes nsc still rejects, including the sealed class that *is*
        // overridden and the block-local class that another class in the same
        // block overrides.
        let name = s.name.clone();
        // A class declared inside a method or a value's right-hand side can
        // only be extended from inside that block, so its subclasses are all
        // in this run -- the same position a `sealed` class is in. A class
        // that is a *member* of an object is not: `object O { class K }` can
        // be extended anywhere, and nsc rejects `@tailrec` on `K`'s members.
        let owner_is_local = matches!(self.st.get(o.owner).kind, SymKind::Method | SymKind::Term);
        if o.flags.contains(Flags::SEALED) || owner_is_local {
            return !self.subclass_overrides(owner, &name);
        }
        false
    }

    /// Does any class in this run extend `owner` (transitively) and declare a
    /// member called `name`? nsc reads the same answer off the sealed parent's
    /// `children`; scanning the symbol table also covers the local-class case,
    /// for which no `children` list is built. Only reached for a `@tailrec`
    /// method in a sealed or block-local class, which is rare enough that the
    /// scan does not need an index.
    fn subclass_overrides(&self, owner: SymbolId, name: &str) -> bool {
        self.st.symbols.iter().any(|s| {
            s.id != owner
                && s.is_class_like()
                && s.members.iter().any(|&m| self.st.get(m).name == name)
                && self.inherits_from(s.id, owner)
        })
    }

    /// Transitive `extends` reachability, following parent *types*.
    fn inherits_from(&self, cls: SymbolId, ancestor: SymbolId) -> bool {
        let mut seen = rustc_hash::FxHashSet::default();
        let mut work = vec![cls];
        while let Some(id) = work.pop() {
            if id.is_none() || !seen.insert(id.0) {
                continue;
            }
            if id == ancestor && id != cls {
                return true;
            }
            for p in &self.st.get(id).parents {
                if let Some(c) = self.st.class_sym_of(p) {
                    work.push(c);
                }
            }
        }
        false
    }

    pub(crate) fn missing_implicit_message(
        &self,
        ty: &Type,
        diverged: Option<(SymbolId, Type)>,
    ) -> String {
        // nsc reports the cut-off expansion rather than a plain "not found"
        // when the search ran into a diverging one.
        if let Some((sym, pt)) = diverged {
            return format!(
                "diverging implicit expansion for type {} starting with method {}",
                self.st.display_type(&pt),
                self.st.get(sym).name
            );
        }
        // nsc does not report a tag the way it reports any other implicit:
        // `Implicits.implicitTagOrOfExpectedType` fails with its own message
        // once the materialiser cannot build one (`neg/classtags_contextbound_a`,
        // `neg/interop_typetags_arenot_classtags`).
        if let Type::Class { sym, args } = ty {
            if self.st.get(*sym).name == "ClassTag" && args.len() == 1 {
                return format!(
                    "No ClassTag available for {}",
                    self.st.display_type(&args[0])
                );
            }
        }
        if let Some(msg) = self.implicit_not_found_msg(ty) {
            return msg;
        }
        format!(
            "no implicit: could not find implicit value of type {}",
            self.st.display_type(ty)
        )
    }

    fn implicit_not_found_msg(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Annotated { tpe, .. } => self.implicit_not_found_msg(tpe),
            Type::Class { sym, args } => self.implicit_not_found_on(*sym, args),
            Type::Named { name, args } => {
                let id = self.st.lookup(name).into_iter().find(|s| {
                    matches!(
                        self.st.get(*s).kind,
                        crate::symbol::SymKind::Class | crate::symbol::SymKind::TypeMember
                    )
                })?;
                self.implicit_not_found_on(id, args)
            }
            _ => None,
        }
    }

    fn implicit_not_found_on(&self, sym: SymbolId, args: &[Type]) -> Option<String> {
        let annots = self.st.get(sym).annotations.clone();
        let tps = self.st.get(sym).tparams.clone();
        for a in &annots {
            let path = a.annotation_path();
            let simple = path.rsplit('.').next().unwrap_or(path.as_str());
            if simple != "implicitNotFound" {
                continue;
            }
            let mut msg = annot_first_string(a)?;
            for (i, tp) in tps.iter().enumerate() {
                let n = self.st.get(*tp).name.clone();
                let shown = args
                    .get(i)
                    .map(|t| self.st.display_type(t))
                    .unwrap_or_else(|| n.clone());
                msg = msg.replace(&format!("${{{n}}}"), &shown);
            }
            return Some(msg);
        }
        None
    }
}
