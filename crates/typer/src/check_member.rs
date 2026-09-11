#![allow(dead_code)]
//! Signatures and bodies of the members inside a template, and the
//! constructor machinery that goes with them.
//!
//! Member signatures are built in their own pass so that bodies can be typed
//! against finished types; a signature that cannot resolve yet is handed back
//! for a second round. Also here: default getters and the parameter defaults
//! they carry, parent applications (`extends C(args)`), primary and auxiliary
//! constructors, and `this(...)` delegation.

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    /// Give the members an `import` in this template selects *through* their
    /// signatures, before the imports are typed.
    ///
    /// nsc's namer gives every definition a lazy completer, so
    /// `import profile.api._` forces `val profile: JdbcProfile` the moment the
    /// import is looked at, wherever the two are written. We type a template
    /// in passes -- imports, then type members, then the rest of the
    /// signatures -- so a prefix that names a `val` of the *same* template was
    /// typed against a symbol that had no type yet, `import_prefix` gave up,
    /// and no wildcard owner was recorded at all. The signature pass builds
    /// each member exactly once (`sig_done`), so the failure was permanent:
    ///
    /// ```scala
    /// trait Profile {
    ///   val profile: BlockingJdbcProfile
    ///   import profile.blockingApi._
    ///   implicit val dateColumnType: BaseColumnType[java.util.Date] = ...
    /// }
    /// ```
    ///
    /// gave "not found: type BaseColumnType", and an implicit whose type is an
    /// error fits *every* implicit search -- which is what gitbucket's ~429
    /// `ambiguous implicit: eventColumnType, dateColumnType` were. Written
    /// with `self: Profile =>` instead, so that `profile` belongs to another
    /// template, the same code resolved.
    ///
    /// Only the head of each prefix, and only a member that states its type:
    /// one that has to infer from its right-hand side is what
    /// `lazysig` is for, and typing it here would move work, not order it.
    pub(crate) fn presig_import_prefixes(&mut self, body: &mut [Tree]) {
        let mut heads: Vec<String> = Vec::new();
        for stt in body.iter() {
            if let TreeKind::Import { expr, .. } = &stt.kind {
                let mut t = &**expr;
                while let TreeKind::Select { qual, .. } = &t.kind {
                    t = qual;
                }
                if let TreeKind::Ident { name } = &t.kind {
                    if !heads.contains(name) {
                        heads.push(name.clone());
                    }
                }
            }
        }
        if heads.is_empty() {
            return;
        }
        for stt in body.iter_mut() {
            let named = match &stt.kind {
                TreeKind::ValDef { name, tpt, .. } | TreeKind::DefDef { name, tpt, .. } => {
                    !tpt.is_empty() && heads.iter().any(|h| h == name)
                }
                _ => false,
            };
            if named {
                self.type_member_sig(stt);
            }
        }
    }

    /// Let the body pass build this member's signature again, when an
    /// `import` above it did not resolve on this pass.
    ///
    /// See [`Typer::sig_rerun_safe`] for why, and for what "again" is safe on.
    /// `mark` is `self.diags.len()` from just before this member's signature
    /// was built: what that build reported is rolled back with it, because
    /// the build itself is being taken back. The round that builds it again
    /// reports whatever is still wrong then.
    ///
    /// The rebuild is a *second signature round* over every unit, not the body
    /// pass, and the difference is the whole point of it. A caller is not
    /// obliged to come after the callee on the command line: gitbucket's
    /// `controller/` sorts before `service/`, so a controller's body is typed
    /// before the service it calls has been rebuilt, and the call still saw
    /// the broken signature. A pass that finishes every unit's signatures
    /// before any body is typed does not depend on that order.
    fn leave_sig_for_body_pass(&mut self, tree: &Tree, mark: usize) {
        if !self.sigs_only
            || self.sig_final_round
            || !self.import_prefix_missed
            || tree.id == scala_rs_parser::NodeId(0)
            || !self.sig_rerun_safe(tree, mark)
        {
            return;
        }
        self.diags.truncate(mark);
        self.sig_deferred = true;
        self.sig_done.remove(&(self.file_index, tree.id));
    }

    /// Type this member's signature, and take the whole attempt back -- the
    /// `sig_done` mark and the diagnostics both -- when an `import` above it
    /// did not resolve on this pass.
    pub(crate) fn type_member_sig_deferrable(&mut self, tree: &mut Tree) {
        let mark = self.diags.len();
        self.type_member_sig(tree);
        self.leave_sig_for_body_pass(tree, mark);
    }

    /// Whether this member's signature can simply be built again.
    ///
    /// A `val`'s signature is its written type and nothing else, so resolving
    /// it a second time in the same scope is idempotent. A `def` was excluded
    /// until now because `type_def_sig` is not: it *appends* the implicit
    /// clause a view or context bound desugars to, so a second run built a
    /// second one and re-typed the first as an ordinary clause whose
    /// parameters have no written type -- slick's
    /// `ShapedValue.mapToImpl[R, U](c: blackbox.Context …)` went from clean to
    /// `not found: type Expr` and 19 more. `drop_synthesized_evidence` removes
    /// that clause on the way in, which is what makes the second run mean the
    /// same as the first. The method's type parameters are *not* reallocated:
    /// `enter_tparams` keeps a `tp.sym` that is already set, so types built
    /// out of the first run still name the parameters the method has. The
    /// default getters `synthesize_default_getters` writes are guarded by a
    /// lookup on the owner and are not written twice.
    ///
    /// Used when an `import` in the enclosing template named a prefix this
    /// pass could not resolve. `import profile.api._` with
    /// `val profile: BlockingJdbcProfile` in *another unit* cannot resolve
    /// during the signature pass -- that unit's own signature pass has not run
    /// yet -- so gitbucket's `implicit val dateColumnType:
    /// BaseColumnType[java.util.Date]` was typed with `BaseColumnType` not in
    /// scope and, `sig_done` being permanent, stayed an error for the rest of
    /// the run. An implicit whose type is an error fits *every* implicit
    /// search. The body pass, by which time every unit has its signatures, is
    /// where such a member should be built.
    ///
    /// The `def` half is gitbucket's `trait AccountFederationComponent {
    /// self: Profile => import profile.api._; def byPrimaryKey(…): Rep[Boolean] }`
    /// -- 36 `not found: type Rep`, and the `(implicit s: Session)` that every
    /// caller of such a service then could not satisfy.
    ///
    /// A `def` is only rebuilt when this attempt actually *reported*
    /// something (`self.diags.len() > mark`). A `val` is rebuilt either way,
    /// which is what root 11 needed and what the numbers were taken on;
    /// widening that to every `def` in every template below the first
    /// unresolved prefix in the run -- `import_prefix_missed` is never
    /// cleared once set -- rebuilt thousands of healthy signatures and cost
    /// more than it gained (measured on gitbucket: the intended 36
    /// `not found: type Rep` went, and 27 `value group is not a member of
    /// Match` and 13 `missing parameter type for expanded function` arrived).
    fn sig_rerun_safe(&self, tree: &Tree, mark: usize) -> bool {
        match &tree.kind {
            TreeKind::ValDef { .. } => true,
            // A constructor's parameters are the class's fields and are shared
            // with `type_class`, which builds them itself; only a plain method
            // is rebuilt here.
            TreeKind::DefDef { name, .. } => {
                name != "<init>" && (self.diags.len() > mark || type_mentions_unresolved(&tree.ty))
            }
            _ => false,
        }
    }

    pub(crate) fn type_member_sig(&mut self, tree: &mut Tree) {
        if self.take_lazy_done(tree) {
            return;
        }
        // Signature work synthesizes evidence parameters and default getters,
        // so it must run exactly once per member even though the signature
        // pass and the body pass both walk the same tree.
        if tree.id != scala_rs_parser::NodeId(0)
            && matches!(tree.kind, TreeKind::ValDef { .. } | TreeKind::DefDef { .. })
            && !self.sig_done.insert((self.file_index, tree.id))
        {
            self.retry_overridden_ret(tree);
            self.refresh_pending_scope(tree);
            return;
        }
        match &tree.kind {
            TreeKind::ValDef { .. } => self.type_val_sig(tree),
            TreeKind::DefDef { .. } => self.type_def_sig(tree),
            TreeKind::ClassDef { .. } => self.type_class(tree),
            TreeKind::ModuleDef { .. } => self.type_module(tree),
            TreeKind::TypeDef { .. } => self.type_type_member(tree),
            _ => {}
        }
        // A macro def's binding is part of its *signature* for the rest of the
        // run: a call site is expanded where it is typed, and the call may
        // stand above the def in the same file. Resolving it in the body pass
        // alone made expansion depend on the order the members are written in.
        // The body pass resolves it again and is what reports; here the
        // diagnostics are rolled back.
        if self.sigs_only
            && matches!(&tree.kind, TreeKind::DefDef { rhs, .. } if matches!(rhs.kind, TreeKind::MacroRhs { .. }))
        {
            let mark = self.diags.len();
            self.type_macro_def(tree);
            self.diags.truncate(mark);
        }
        if matches!(
            &tree.kind,
            TreeKind::ValDef { .. } | TreeKind::DefDef { .. }
        ) {
            self.register_typed_sig(tree);
        }
    }

    /// Retry the inherited result expectation for an unannotated implementation.
    ///
    /// `overridden_ret_type` deliberately does not force a candidate's
    /// signature (doing so measured 155 slick errors -> 307), so it can only
    /// read one that is already known. During the signature pass that is a
    /// matter of command-line order: `memory/DistributedProfile.scala`'s
    /// `override def run(n: Node) = … run(from) …` is walked before
    /// `memory/QueryInterpreter.scala`, whose `def run(n: Node): Any` states
    /// the very type it wants, so the search found nothing and the method
    /// stayed inference-bound -- and its own self-call then reported
    /// `recursive method run needs result type`. By the body pass every
    /// written signature is in place, so the search is simply run again on a
    /// method that is *still* without a result type. Nothing else about the
    /// signature is redone: no evidence parameter or default getter is
    /// synthesized twice.
    pub(crate) fn retry_overridden_ret(&mut self, tree: &mut Tree) {
        if self.sigs_only {
            return;
        }
        let TreeKind::DefDef {
            mods, name, tpt, ..
        } = &tree.kind
        else {
            return;
        };
        if !tpt.is_empty()
            || name == "<init>"
            || self.block_local_defs.contains(&(self.file_index, tree.id))
        {
            return;
        }
        let abstract_only = !mods.flags.contains(Flags::OVERRIDE);
        let name = name.clone();
        let sym = tree.sym;
        if sym.is_none() {
            return;
        }
        let Type::Method { paramss, ret } = self.st.get(sym).ty.clone() else {
            return;
        };
        if !ret.is_no_type() {
            return;
        }
        let owner = self.st.get(sym).owner;
        let Some(found) = self.overridden_ret_type(
            owner,
            &name,
            &paramss,
            &self.st.get(sym).tparams.clone(),
            &self.st.get(sym).params.clone(),
            abstract_only,
        ) else {
            return;
        };
        if found.is_no_type() || found.is_error() {
            return;
        }
        let mty = Type::Method {
            paramss,
            ret: Box::new(found),
        };
        tree.ty = mty.clone();
        self.st.get_mut(sym).ty = mty;
        // With a result type there is nothing left to infer, so the method is
        // no longer lazy: leaving it pending would make its own recursive call
        // re-enter `complete_lazy_sig` and report the cycle this just removed.
        self.drop_lazy_sig(sym);
    }

    pub(crate) fn type_member_body(&mut self, tree: &mut Tree) {
        if self.take_lazy_done(tree) {
            return;
        }
        match &tree.kind {
            TreeKind::ValDef { .. } => self.type_val_body(tree),
            TreeKind::DefDef { .. } => self.type_def_body(tree),
            TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } | TreeKind::TypeDef { .. } => {
                self.check_stored_annotations(tree);
            }
            TreeKind::Import { .. } => self.type_import(tree),
            _ => {
                self.type_stat(tree);
            }
        }
    }

    pub(crate) fn type_val_sig(&mut self, tree: &mut Tree) {
        let local = self.block_local_defs.contains(&(self.file_index, tree.id));
        let (tpt, name, flags, within) = match &tree.kind {
            TreeKind::ValDef {
                tpt, name, mods, ..
            } => (
                tpt.clone(),
                name.clone(),
                mods.flags,
                mods.private_within.clone(),
            ),
            _ => return,
        };
        let ty = if tpt.is_empty() {
            if !tree.ty.is_no_type() {
                tree.ty.clone()
            } else {
                Type::NoType
            }
        } else {
            // A written type annotation is a name nsc has finished resolving:
            // `def f(x: Zork)` and `val x: Zork` are `not found: type Zork`,
            // not a silently accepted program. See `strict_type_names`.
            let saved_cto = std::mem::replace(&mut self.cto_sig_owner, tree.sym);
            let ty = self.with_strict_sig_names(|s| s.tree_to_type(&tpt));
            self.cto_sig_owner = saved_cto;
            self.check_proper_type(&ty, tree.span);
            ty
        };
        let ty = if flags.contains(Flags::BYNAME) && !matches!(ty, Type::ByName(_) | Type::NoType) {
            Type::ByName(Box::new(ty))
        } else {
            ty
        };
        tree.ty = ty.clone();
        if tree.sym.is_none() {
            let id = self
                .st
                .alloc(name.clone(), self.st.owner, SymKind::Term, flags, "");
            tree.sym = id;
            self.st.enter_in_current(&name, id);
        }
        if local && !tree.sym.is_none() {
            let owner = self.st.get(tree.sym).owner;
            if !owner.is_none() && self.st.get(owner).is_class_like() {
                self.st.get_mut(owner).members.retain(|m| *m != tree.sym);
            }
        }
        if !tree.sym.is_none() {
            self.st.get_mut(tree.sym).ty = ty.clone();
            self.st.get_mut(tree.sym).private_within = within;
            if flags.contains(Flags::DEFAULTPARAM) {
                let f = self.st.get(tree.sym).flags.with(Flags::DEFAULTPARAM);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::IMPLICIT) {
                let f = self.st.get(tree.sym).flags.with(Flags::IMPLICIT);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::BYNAME) {
                let f = self.st.get(tree.sym).flags.with(Flags::BYNAME);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::LOCAL) {
                let f = self.st.get(tree.sym).flags.with(Flags::LOCAL);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::PRIVATE) {
                let f = self.st.get(tree.sym).flags.with(Flags::PRIVATE);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::PROTECTED) {
                let f = self.st.get(tree.sym).flags.with(Flags::PROTECTED);
                self.st.get_mut(tree.sym).flags = f;
            }
        }
        let _ = name;
    }

    /// A value's type is settled here, so whatever its right-hand side left
    /// undetermined stops being a variable: a *later* statement must not be
    /// able to instantiate it.
    ///
    /// nsc keeps `undetparams` in the typer context, which is per definition.
    /// scala-rs keeps one list on the typer, saved and restored around each
    /// application (`type_apply`) but around nothing else, so a variable that
    /// reached a definition's result with nothing to pin it stayed open for
    /// the rest of the block. With `def boom[A](m: String): Box[A]`,
    ///
    /// ```scala
    /// val x = good.fold(boom, _ => new Box(()))
    /// val bad: Box[Unit] = x
    /// ```
    ///
    /// the second line solved the *first* line's `A`, and a program nsc
    /// rejects compiled. (nsc infers `Box[_1] forSome { type _1 <: Unit }`
    /// for `x` and reports the mismatch.) Dropping the variables here leaves
    /// `x`'s type naming `A` as the fixed type it now is, which is what the
    /// mismatch describes.
    pub(crate) fn type_val_body(&mut self, tree: &mut Tree) {
        let saved = self.macro_enter_owner(tree.sym);
        self.type_val_body_with_macro_owner(tree);
        self.macro_lexical_owner = saved;
    }

    fn type_val_body_with_macro_owner(&mut self, tree: &mut Tree) {
        let saved = std::mem::take(&mut self.undet_tvars);
        self.type_val_body_in(tree);
        self.undet_tvars = saved;
    }

    fn warn_trivial_self_reference(&mut self, sym: SymbolId, rhs: &Tree) {
        if sym.is_none() || rhs.ty.is_error() {
            return;
        }
        let member = self.st.get(sym);
        if member.flags.contains(Flags::PARAM) || member.paramss.iter().any(|ps| !ps.is_empty()) {
            return;
        }
        let reference = match &rhs.kind {
            TreeKind::Ident { .. } => Some(rhs),
            TreeKind::Select { qual, .. } if matches!(qual.kind, TreeKind::This { .. }) => {
                Some(rhs)
            }
            TreeKind::Apply { fun, args } if args.is_empty() => match &fun.kind {
                TreeKind::Select { qual, .. } if matches!(qual.kind, TreeKind::This { .. }) => {
                    Some(&**fun)
                }
                TreeKind::Ident { .. } if self.st.get(member.owner).is_class_like() => Some(&**fun),
                _ => None,
            },
            _ => None,
        };
        if reference.is_none_or(|r| r.sym != sym) {
            return;
        }
        let kind = if member.kind == SymKind::Method {
            "method"
        } else {
            "value"
        };
        let message = format!(
            "{kind} {} does nothing other than call itself recursively",
            member.name
        );
        if !self
            .diags
            .iter()
            .any(|d| d.file_index == self.file_index && d.span == rhs.span && d.message == message)
        {
            self.warning(rhs.span, message);
        }
    }

    /// nsc's `widenIfNecessary` for a `var` or a method: an inferred type is
    /// never a singleton. `private[this] var origElems = self` in
    /// `Iterator.patch` is an `Iterator[A]`, not `Iterator.this.type` -- or
    /// `origElems = origElems drop replaced` could not assign to it -- and
    /// `def f = addOne(x)` is the class, not `this.type`. A `val` keeps the
    /// singleton (`val c = b.append(1)` is a `b.type`), and a module's type is
    /// never widened (`Infer Foo.type instead of "object Foo"`).
    pub(crate) fn widen_inferred_singleton(&self, ty: Type) -> Type {
        let mut ty = ty;
        for _ in 0..8 {
            ty = match ty {
                Type::ThisType(c) if !c.is_none() => self.st.self_type_of_class(c),
                Type::SingleType { sym, .. } => {
                    let under = self.st.singleton_underlying(sym);
                    if under.is_no_type() || matches!(under, Type::Method { .. }) {
                        return ty;
                    }
                    under
                }
                other => return other,
            };
        }
        ty
    }

    fn type_val_body_in(&mut self, tree: &mut Tree) {
        let feature = self
            .source_features
            .contains(crate::source_features::SourceFeature::InferOverride)
            && self.macro_depth == 0;
        let (inherited, final_value) = match &tree.kind {
            TreeKind::ValDef {
                tpt, mods, name, ..
            } if tpt.is_empty()
                && !tree.sym.is_none()
                && !self.block_local_defs.contains(&(self.file_index, tree.id)) =>
            {
                let owner = self.st.get(tree.sym).owner;
                (
                    self.overridden_ret_type(
                        owner,
                        name,
                        &[],
                        &[],
                        &[],
                        !feature && !mods.flags.contains(Flags::OVERRIDE),
                    ),
                    mods.flags.contains(Flags::FINAL),
                )
            }
            _ => (None, false),
        };
        let presuper = matches!(
            &tree.kind,
            TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::PRESUPER)
        );
        let is_var = matches!(
            &tree.kind,
            TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::MUTABLE)
        );
        let (rhs, declared, literal_tpt) = match &mut tree.kind {
            TreeKind::ValDef { rhs, tpt, .. } => {
                let literal = match &tpt.kind {
                    TreeKind::Literal { lit } if !matches!(lit, Lit::Unit) => Some(tpt.span),
                    _ => None,
                };
                (rhs, tree.ty.clone(), literal)
            }
            _ => return,
        };
        if rhs.is_empty() {
            if declared.is_no_type() {
                self.error(tree.span, "abstract value needs a type");
                tree.ty = Type::Error;
            }
            return;
        }
        // `var x: T = _` (nsc's DEFAULTINIT, SLS 4.2): the field starts at
        // the JVM default of its erased type and the constructor never
        // stores to it -- so a value a superclass constructor wrote through
        // an overridden method survives, and any `T` qualifies, a type
        // parameter or a value class included. There is no expression to
        // type: the `_` stays in the tree, typed as `T`, and the backend
        // skips the initializer (`Tree::is_default_init`).
        if rhs.is_default_init() {
            if declared.is_no_type() {
                self.error(tree.span, "unbound placeholder parameter");
                tree.ty = Type::Error;
                return;
            }
            // nsc `LocalVarUninitializedError`: a local variable has no
            // default value.
            if self.block_local_defs.contains(&(self.file_index, tree.id)) {
                self.error(tree.span, "local variables must be initialized");
                rhs.ty = declared.clone();
                tree.ty = declared;
                return;
            }
            // SIP-23: a literal type has one value and no default. nsc asks
            // the type *as written* -- `var x: One = _` through `type One =
            // 1` is accepted -- so this reads the tree, not the dealiased
            // type (`neg/sip23-uninitialized-2`).
            if let Some(span) = literal_tpt {
                self.error(
                    span,
                    "default initialization prohibited for literal-typed vars",
                );
                rhs.ty = declared.clone();
                tree.ty = declared;
                return;
            }
            rhs.ty = declared.clone();
            tree.ty = declared;
            return;
        }
        let pt = inherited.clone().unwrap_or_else(|| declared.clone());
        // A definition owns its inference boundary, including in a by-name
        // argument whose enclosing application is still being inferred.
        let saved_call_args = std::mem::take(&mut self.typing_call_args);
        // An early definition is typed in the constructor context, outside
        // the template (see `crate::presuper`).
        let ctor_ctx = if presuper {
            self.enter_presuper_scope(tree.sym)
        } else {
            None
        };
        self.type_expr(rhs, &pt);
        // An inferred value has no expected type to trigger adapt's backstop.
        // A missing implicit is still an error, not a function to eta-expand.
        // Nor is a method with explicit parameters: nsc's "missing argument
        // list", or its eta-expansion under `-Xsource:3`.
        if pt.is_no_type() && !self.reject_unapplied_implicit_clause(rhs) {
            self.adapt_method_value(rhs);
        }
        if let Some(saved) = ctor_ctx {
            self.leave_presuper_scope(saved);
        }
        self.typing_call_args = saved_call_args;
        // A strict *local* value may not mention itself in its own
        // initializer (nsc refchecks: "forward reference extends over
        // definition of value x"): the slot is read before it is written.
        // `val fibs: LazyList[BigInt] = 0 #:: fibs.zip(...)` was accepted and
        // read `null`. A field reads its default instead, and a `lazy val`
        // is initialized on first use, so both stay legal.
        if !tree.sym.is_none()
            && self.block_local_defs.contains(&(self.file_index, tree.id))
            && !self.st.get(tree.sym).flags.contains(Flags::LAZY)
            && tree_mentions_sym(rhs, tree.sym)
        {
            let name = self.st.get(tree.sym).name.clone();
            self.error(
                tree.span,
                format!("forward reference extends over definition of value {name}"),
            );
        }
        self.warn_trivial_self_reference(tree.sym, rhs);
        let preserve_constant = final_value && matches!(rhs.ty, Type::Constant(_));
        if let Some(expected) = inherited.filter(|_| feature && !preserve_constant) {
            self.adapt(rhs, &expected);
            tree.ty = expected;
            self.st.get_mut(tree.sym).ty = tree.ty.clone();
        } else if declared.is_no_type() {
            // A variable the right-hand side left undetermined is closed here
            // (nsc's mono-mode `instantiate`): `val b = List.newBuilder` is a
            // `Builder[Nothing, List[Nothing]]`, not a `Builder[?A, …]` that a
            // later line could still solve.
            tree.ty = self.close_leaked_undet(&rhs.ty.widen_constant());
            if is_var {
                tree.ty = self.widen_inferred_singleton(tree.ty.clone());
            }
            if !tree.sym.is_none() {
                self.st.get_mut(tree.sym).ty = tree.ty.clone();
            }
        } else {
            self.adapt(rhs, &declared);
            tree.ty = declared;
        }
        // The signature is settled; nothing may complete this value again.
        // Unlike a method it is *not* locked while its own right-hand side is
        // typed: scalac reports `val x = y; val y = x` on the reference to `y`.
        self.drop_lazy_sig(tree.sym);
        self.check_stored_annotations(tree);
    }

    pub(crate) fn type_def_sig(&mut self, tree: &mut Tree) {
        let local = self.block_local_defs.contains(&(self.file_index, tree.id));
        let span = tree.span;
        Self::drop_synthesized_evidence(tree);
        let (tparams, vparamss, tpt, name, mods_within, mods_flags, is_conv) = match &mut tree.kind
        {
            TreeKind::DefDef {
                tparams,
                vparamss,
                tpt,
                name,
                mods,
                ..
            } => {
                let is_conv = mods.flags.contains(Flags::IMPLICIT)
                    && !mods.flags.contains(Flags::SYNTHETIC)
                    && is_implicit_conversion_shape(vparamss);
                (
                    tparams,
                    vparamss,
                    tpt.clone(),
                    name.clone(),
                    mods.private_within.clone(),
                    mods.flags,
                    is_conv,
                )
            }
            _ => return,
        };
        if is_conv {
            self.check_implicit_conversions_feature(span, &name);
        }
        if tree.sym.is_none() {
            let id = self.st.alloc(
                name.clone(),
                self.st.owner,
                SymKind::Method,
                Flags::EMPTY,
                "",
            );
            tree.sym = id;
            self.st.enter_in_current(&name, id);
        }
        self.st.get_mut(tree.sym).parameterless_method = Some(vparamss.is_empty());
        if local {
            let owner = self.st.get(tree.sym).owner;
            if !owner.is_none() && self.st.get(owner).is_class_like() {
                self.st.get_mut(owner).members.retain(|m| *m != tree.sym);
            }
        }
        self.st.push_scope();
        let tp_ids = self.enter_tparams(tparams, tree.sym);
        self.st.get_mut(tree.sym).tparams = tp_ids.clone();
        let mut view_work: Vec<(SymbolId, Vec<Tree>, Span, bool)> = Vec::new();
        let mut ctx_work: Vec<(SymbolId, Vec<Tree>, Span, bool)> = Vec::new();
        let mut bound_work: Vec<(SymbolId, Option<Tree>, Option<Tree>)> = Vec::new();
        for (i, tp) in tparams.iter().enumerate() {
            if let TreeKind::TypeDef {
                views,
                ctx_bounds,
                tparams: inner,
                lo,
                hi,
                ..
            } = &tp.kind
            {
                let hk = !inner.is_empty();
                if !views.is_empty() {
                    view_work.push((tp_ids[i], views.clone(), tp.span, hk));
                }
                if !ctx_bounds.is_empty() {
                    ctx_work.push((tp_ids[i], ctx_bounds.clone(), tp.span, hk));
                }
                if lo.is_some() || hi.is_some() {
                    bound_work.push((
                        tp_ids[i],
                        lo.as_ref().map(|t| (**t).clone()),
                        hi.as_ref().map(|t| (**t).clone()),
                    ));
                }
            }
        }
        // `[B >: A]` / `[A <: Named]`: remember the bounds on the type parameter
        // symbol so inference can widen to the lower bound and check the upper one.
        for (tp_id, lo, hi) in bound_work {
            let lo_ty = lo.map(|t| self.tree_to_type(&t));
            let hi_ty = hi.map(|t| self.tree_to_type(&t));
            if let Some(t) = lo_ty {
                if !t.is_error() {
                    self.st.get_mut(tp_id).bound_lo = Some(t);
                }
            }
            if let Some(t) = hi_ty {
                if !t.is_error() {
                    self.st.get_mut(tp_id).bound_hi = Some(t);
                }
            }
        }
        // `def g[X, A[X] <: A[X]](x: A[X])` (`neg/t2918`) is rejected here.
        let bound_ids: Vec<(SymbolId, Span)> = tparams
            .iter()
            .zip(tp_ids.iter())
            .map(|(tp, id)| (*id, tp.span))
            .collect();
        self.report_bound_cycles(&bound_ids);
        let saved_owner = self.st.owner;
        self.st.owner = tree.sym;
        let mut paramss_ty = Vec::new();
        let mut all_params = Vec::new();
        let mut paramss_ids: Vec<Vec<SymbolId>> = Vec::new();
        if name == "<init>" {
            // Class bounds introduce fresh evidence on every auxiliary
            // constructor, before any explicitly written implicit parameters.
            // Reusing the primary constructor's fields would read this before
            // initialization and would hide ambiguities with explicit evidence.
            let bound_types = self
                .class_bound_evidence_types
                .get(&saved_owner)
                .cloned()
                .unwrap_or_default();
            let mut evidence = Vec::new();
            for ty in bound_types {
                self.gensym += 1;
                let name = format!("evidence${}", self.gensym);
                let flags = Flags::IMPLICIT.with(Flags::PARAM).with(Flags::SYNTHETIC);
                let id = self.st.alloc(&name, tree.sym, SymKind::Term, flags, "");
                self.st.get_mut(id).ty = ty.clone();
                self.st.enter_in_current(&name, id);
                let mut param = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(flags),
                    name,
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                param.span = span;
                param.sym = id;
                param.ty = ty;
                evidence.push(param);
            }
            if !evidence.is_empty() {
                if let Some(clause) = vparamss.last_mut().filter(|clause| {
                    clause.first().is_some_and(|p| {
                        matches!(&p.kind,
                        TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::IMPLICIT))
                    })
                }) {
                    evidence.append(clause);
                    *clause = evidence;
                } else {
                    vparamss.push(evidence);
                }
            }
        }
        for clause in vparamss.iter_mut() {
            let mut ct = Vec::new();
            let mut ids = Vec::new();
            let last_in_clause = clause.len().saturating_sub(1);
            for (pi, p) in clause.iter_mut().enumerate() {
                self.type_val_sig(p);
                // nsc, at this parameter's own position. A repeated parameter
                // covers every argument from its position on, so one that is
                // not last has no meaning; `def f(xs: Int*, y: Int)` used to
                // be accepted, and the call `f(1, 2)` then had to guess which
                // argument was `y`. The rule is per *clause*: scalac 2.13.16
                // accepts `def g(xs: Int*)(y: Int)`.
                if matches!(p.ty, Type::Repeated(_)) && pi != last_in_clause {
                    self.error(p.span, "*-parameter must come last");
                }
                if p.ty.is_no_type() {
                    self.error(
                        p.span,
                        format!("missing parameter type for `{}`", p.name().unwrap_or("?")),
                    );
                    p.ty = Type::Error;
                }
                if p.sym.is_none() {
                    if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                        let n = name.clone();
                        let flags = mods.flags.with(Flags::PARAM);
                        let id = self.st.alloc(n, tree.sym, SymKind::Term, flags, "");
                        p.sym = id;
                    }
                }
                if !p.sym.is_none() {
                    // nsc: `xs: T*` is a `Seq[T]` inside the body; the method
                    // type keeps `Repeated` so call sites still wrap arguments.
                    let sym_ty = match &p.ty {
                        Type::Repeated(inner) => self
                            .seq_of(inner)
                            .unwrap_or_else(|| Type::Repeated(inner.clone())),
                        other => other.clone(),
                    };
                    self.st.get_mut(p.sym).ty = sym_ty;
                    if let TreeKind::ValDef { mods, rhs, .. } = &p.kind {
                        if mods.flags.contains(Flags::DEFAULTPARAM) && !rhs.is_empty() {
                            self.st.get_mut(p.sym).default_rhs = Some((**rhs).clone());
                            // A *secondary* constructor's default is evaluated
                            // before the instance exists, exactly like the
                            // primary's, so the class's own member scope is not
                            // available to it. nsc puts the getter on the
                            // companion and reports `not found: value f` for
                            // `class T(v: String) { val f = "F"
                            //   def this(n: Int, s: String = f) = this(s + n) }`.
                            // Leaving `this` in resolved `f` to the *field* and
                            // spliced a read off the caller's own `this`:
                            // `ClassCastException: Main$ cannot be cast to T`.
                            // Same reasoning as `check_template`'s primary-
                            // constructor call; see `record_default_scope`.
                            if name == "<init>" {
                                self.record_secondary_ctor_default_scope(p.sym);
                            } else {
                                self.record_default_scope(p.sym, true);
                            }
                        }
                    }
                    all_params.push(p.sym);
                    ids.push(p.sym);
                }
                ct.push(p.ty.clone());
            }
            paramss_ty.push(ct);
            paramss_ids.push(ids);
        }
        // nsc: `T <% V` becomes an extra implicit clause `(implicit evidence$n: T => V)`.
        let mut evidence = Vec::new();
        for (tp_id, views, span, hk) in view_work {
            if hk {
                let tp_name = self.st.get(tp_id).name.clone();
                self.error(span, format!("type {tp_name} takes type parameters"));
                continue;
            }
            for view in views {
                if matches!(
                    view.kind,
                    TreeKind::ExistentialTypeTree { .. }
                        | TreeKind::CompoundTypeTree { .. }
                        | TreeKind::Unimplemented { .. }
                ) {
                    self.error(
                        view.span,
                        "unimplemented syntax: view bound shape (existential/refinement)",
                    );
                    continue;
                }
                let view_ty = self.tree_to_type(&view);
                if view_ty.is_error() {
                    continue;
                }
                self.gensym += 1;
                let ev_name = format!("evidence${}", self.gensym);
                let ev_ty = Type::Function {
                    params: vec![Type::TypeParam(tp_id)],
                    ret: Box::new(view_ty),
                };
                let ev_id = self.st.alloc(
                    &ev_name,
                    tree.sym,
                    crate::symbol::SymKind::Term,
                    Flags::IMPLICIT.with(Flags::PARAM),
                    "",
                );
                self.st.get_mut(ev_id).ty = ev_ty.clone();
                self.st.enter_in_current(&ev_name, ev_id);
                let mut ev = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(Flags::IMPLICIT.with(Flags::PARAM)),
                    name: ev_name,
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                ev.span = span;
                ev.sym = ev_id;
                ev.ty = ev_ty.clone();
                evidence.push(ev);
                all_params.push(ev_id);
            }
        }
        for (tp_id, bounds, span, _hk) in ctx_work {
            // nsc 2.13.16 accepts a context bound on a higher-kinded parameter
            // (`def f[F[_]: Async]` desugars to `(implicit ev: Async[F])`); only
            // *view* bounds on such a parameter are rejected.
            for bound in bounds {
                if matches!(
                    bound.kind,
                    TreeKind::ExistentialTypeTree { .. }
                        | TreeKind::CompoundTypeTree { .. }
                        | TreeKind::Unimplemented { .. }
                ) {
                    self.error(
                        bound.span,
                        "unimplemented syntax: context bound shape (existential/refinement)",
                    );
                    continue;
                }
                let bound_ty = self.tree_to_type(&bound);
                if bound_ty.is_error() {
                    continue;
                }
                let ev_ty = self.expand_bound_evidence(apply_context_bound(bound_ty, tp_id));
                self.gensym += 1;
                let ev_name = format!("evidence${}", self.gensym);
                let ev_id = self.st.alloc(
                    &ev_name,
                    tree.sym,
                    crate::symbol::SymKind::Term,
                    Flags::IMPLICIT.with(Flags::PARAM),
                    "",
                );
                self.st.get_mut(ev_id).ty = ev_ty.clone();
                self.st.enter_in_current(&ev_name, ev_id);
                let mut ev = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(Flags::IMPLICIT.with(Flags::PARAM)),
                    name: ev_name,
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                ev.span = span;
                ev.sym = ev_id;
                ev.ty = ev_ty.clone();
                evidence.push(ev);
                all_params.push(ev_id);
            }
        }
        if !evidence.is_empty() {
            let tys: Vec<Type> = evidence.iter().map(|e| e.ty.clone()).collect();
            let ids: Vec<SymbolId> = evidence.iter().map(|e| e.sym).collect();
            paramss_ty.push(tys);
            paramss_ids.push(ids);
            vparamss.push(evidence);
        }
        // A *secondary* constructor's defaults are not ordinary method
        // defaults. `synthesize_default_getters` would declare an instance
        // method `<init>$default$n` on the class being constructed, and
        // `new C(x)` has no receiver to select it off: `default_getter_apply`
        // found that member, fell through to `default_getter_receiver`, and
        // built `Main$.<init>$default$2()` -- "value <init>$default$2 is not a
        // member of Main$". nsc puts a constructor's getters on the companion
        // module under the JVM spelling instead, which is what
        // `synthesize_ctor_default_getters` does for every constructor,
        // primary and secondary alike (`crate::ctor_defaults`).
        if name == "apply" {
            self.unlink_suppressed_case_apply(saved_owner, tree.sym, &tp_ids, &paramss_ty);
        }
        if name != "<init>" {
            self.synthesize_default_getters(saved_owner, tree.sym, &name, &tp_ids, &paramss_ids);
        }
        self.st.owner = saved_owner;
        let ret = if name == "<init>" {
            Type::Unit
        } else if tpt.is_empty() && local {
            Type::NoType
        } else if tpt.is_empty() {
            // A known inherited result guides the body and permits recursive
            // references before the final, possibly narrower result is inferred.
            // Implementing an abstract member does not require a written
            // override modifier. Reuse known abstract signatures in that case,
            // without forcing pending source bodies to complete out of order.
            self.overridden_ret_type(
                saved_owner,
                &name,
                &paramss_ty,
                &tp_ids,
                &all_params,
                !mods_flags.contains(Flags::OVERRIDE),
            )
            .unwrap_or(Type::NoType)
        } else {
            // As in `type_val_sig`: a written result type is fully resolvable
            // by the time nsc looks at it, so an unresolved name is an error
            // and not a placeholder.
            let saved_cto = std::mem::replace(&mut self.cto_sig_owner, tree.sym);
            let ret = self.with_strict_sig_names(|s| s.tree_to_type(&tpt));
            self.cto_sig_owner = saved_cto;
            self.check_proper_type(&ret, span);
            ret
        };
        if name == "<init>" && !tree.sym.is_none() {
            let f = self.st.get(tree.sym).flags.with(Flags::CONSTRUCTOR);
            self.st.get_mut(tree.sym).flags = f;
        }
        self.st.pop_scope();
        let mty = Type::Method {
            paramss: paramss_ty,
            ret: Box::new(ret.clone()),
        };
        tree.ty = mty.clone();
        if !tree.sym.is_none() {
            self.st.get_mut(tree.sym).ty = mty;
            self.st.get_mut(tree.sym).params = all_params;
            self.st.get_mut(tree.sym).paramss = paramss_ids;
            self.st.get_mut(tree.sym).private_within = mods_within;
            if mods_flags.contains(Flags::LOCAL) {
                let f = self.st.get(tree.sym).flags.with(Flags::LOCAL);
                self.st.get_mut(tree.sym).flags = f;
            }
            if mods_flags.contains(Flags::PRIVATE) {
                let f = self.st.get(tree.sym).flags.with(Flags::PRIVATE);
                self.st.get_mut(tree.sym).flags = f;
            }
            if mods_flags.contains(Flags::PROTECTED) {
                let f = self.st.get(tree.sym).flags.with(Flags::PROTECTED);
                self.st.get_mut(tree.sym).flags = f;
            }
            // A class/module member's `implicit` is already on the symbol --
            // the namer pre-allocates it (`namer_member`) with the full flag
            // set before `type_def_sig` ever runs. A *local* `def` inside a
            // block has no such namer pass: `tree.sym.is_none()` above
            // allocates it fresh with `Flags::EMPTY`, and until now nothing
            // ever copied `implicit` from the modifiers onto that symbol. Every
            // implicit search (`implicits_in_scope`, used by both
            // `search_implicit` for implicit *parameters* and by
            // `search_conversion` / `search_extension` for views) filters
            // candidates on `Flags::IMPLICIT`, so a local `implicit def` used
            // as a view or an extension source was silently invisible even
            // though it was correctly entered into the block's scope.
            if mods_flags.contains(Flags::IMPLICIT) {
                let f = self.st.get(tree.sym).flags.with(Flags::IMPLICIT);
                self.st.get_mut(tree.sym).flags = f;
            }
        }
        let _ = name;
    }

    /// Undo the one edit `type_def_sig` makes to its own input.
    ///
    /// View and context bounds synthesize implicit parameters, either in an
    /// extra clause or before an auxiliary constructor's written implicits.
    /// Building the same signature a second time --
    /// which `leave_sig_for_body_pass` asks for when an `import` above the
    /// method did not resolve on the signature pass -- would append a second
    /// copy, and worse, re-type the first as an ordinary clause: a synthesized
    /// evidence parameter carries its type on the symbol and writes none in
    /// the tree, so `type_def_sig` would report `missing parameter type for
    /// evidence$1` for a bound the source never wrote. The bounds themselves
    /// live on the type parameters and are untouched, so dropping the clause
    /// here loses nothing -- it is rebuilt from `view_work` / `ctx_work`, or
    /// from the enclosing class's recorded bound evidence.
    ///
    /// A parameter with no written type cannot occur in source, so the shape
    /// identifies the clause on its own.
    fn drop_synthesized_evidence(tree: &mut Tree) {
        let TreeKind::DefDef { vparamss, .. } = &mut tree.kind else {
            return;
        };
        vparamss.retain_mut(|clause| {
            let was_empty = clause.is_empty();
            clause.retain(|p| {
                !matches!(&p.kind,
                TreeKind::ValDef { name, tpt, .. }
                    if name.starts_with("evidence$") && tpt.is_empty())
            });
            was_empty || !clause.is_empty()
        });
    }

    pub(crate) fn synthesize_default_getters(
        &mut self,
        owner: SymbolId,
        _meth: SymbolId,
        name: &str,
        tp_ids: &[SymbolId],
        paramss_ids: &[Vec<SymbolId>],
    ) {
        if owner.is_none() || name.contains("$default$") {
            return;
        }
        let flat: Vec<SymbolId> = paramss_ids.iter().flatten().copied().collect();
        for (i, pid) in flat.iter().enumerate() {
            if !self.st.get(*pid).flags.contains(Flags::DEFAULTPARAM) {
                continue;
            }
            let n = i + 1;
            let gname = format!("{name}$default${n}");
            if self
                .st
                .lookup_member(owner, &gname)
                .iter()
                .any(|&id| self.st.get(id).name == gname)
            {
                continue;
            }
            // nsc's getter takes the *preceding parameter clauses* only:
            // `def f(x: Int)(y: Int = x)` gives `f$default$2(x: Int)`, but
            // `def f(x: Int, y: Int = 0, z: Int = 1)` gives a **nullary**
            // `f$default$2()` / `f$default$3()`, because a default may not
            // name an earlier parameter of its own clause (nsc rejects
            // `def d(x: Int, z: Int = x)` outright).
            //
            // Taking the whole flattened prefix instead made every call that
            // omits k defaults duplicate the argument trees 2^k times: the
            // arguments go into the call *and* into `$default$2`, and
            // `$default$3` then takes both of those. slick's
            // `sel.replace { case … }` (two omitted defaults) emitted the one
            // `PartialFunction` literal as **16** classfiles.
            //
            // scala-rs still accepts the same-clause reference nsc rejects, so
            // those parameters are kept when the default body actually names
            // one -- the exponent is what had to go, not the capability.
            let clause_start = clause_start_of(paramss_ids, i);
            let same_clause = &flat[clause_start..i];
            let keep_same_clause = !same_clause.is_empty()
                && self.st.get(*pid).default_rhs.as_ref().is_some_and(|rhs| {
                    let names: Vec<String> = same_clause
                        .iter()
                        .map(|id| self.st.get(*id).name.clone())
                        .collect();
                    tree_names_any(rhs, &names)
                });
            let cut = if keep_same_clause { i } else { clause_start };
            let preceding: Vec<SymbolId> = flat[..cut].to_vec();
            let preceding_tys: Vec<Type> = preceding
                .iter()
                .map(|id| self.st.get(*id).ty.clone())
                .collect();
            let ret = self.st.get(*pid).ty.clone();
            // A primary constructor declares no type parameters of its own:
            // `class C[A](…)`'s `A` belongs to the *class*, and the `<init>`
            // this runs for hands over an empty `tp_ids`. The default body is
            // typed in a scope of its own, so `A` was bound to nothing there
            // and `class C[A](val l: List[A] = List.empty[A])` reported
            // `found: List[A] required: List[A]` -- the found `A` being an
            // unresolved *name*. Whatever the parameter types are written in
            // is in scope for the body that has to produce one.
            let rhs_tparams = self.tparams_in_scope_for_default(tp_ids, &ret, &preceding_tys);
            let rhs_tparams = &rhs_tparams[..];
            let gid = self.st.alloc(
                &gname,
                owner,
                crate::symbol::SymKind::Method,
                Flags::SYNTHETIC,
                "",
            );
            // With nothing preceding it the getter is *nullary* -- nsc's
            // `def copy$default$1: Int`, a `NullaryMethodType` -- and not a
            // method over an empty list. The two are different types to a
            // reader: pickled as `copy$default$1()`, scalac warns
            // "Auto-application to `()` is deprecated" at every call that
            // omits a `copy` argument, and Scala 3 would eta-expand it.
            let getter_paramss = if preceding.is_empty() {
                Vec::new()
            } else {
                vec![preceding.clone()]
            };
            self.st.get_mut(gid).ty = Type::Method {
                paramss: if preceding_tys.is_empty() {
                    Vec::new()
                } else {
                    vec![preceding_tys]
                },
                ret: Box::new(ret.clone()),
            };
            self.st.get_mut(gid).params = preceding.clone();
            self.st.get_mut(gid).paramss = getter_paramss;
            self.st.get_mut(gid).tparams = tp_ids.to_vec();
            if self.st.get(*pid).default_rhs.is_some() {
                if self.defer_default_rhs {
                    self.defer_default_getter_rhs(*pid, gid, &ret, rhs_tparams, &preceding);
                } else {
                    self.type_default_getter_rhs(*pid, gid, &ret, rhs_tparams, &preceding);
                }
            }
        }
    }

    /// Type a default's stored body for a call that omitted the argument.
    ///
    /// A primary constructor's defaults have no `name$default$n` getters (see
    /// `namer_tmpl`: there is no receiver to call one on), so the tree the
    /// namer stored is typed here rather than in a getter body.
    ///
    /// **Where** it is typed is the whole point. `record_default_scope` kept
    /// the scope stack of the definition, and it is swapped back in here: a
    /// default's right-hand side means what it meant where it was written, not
    /// what its names happen to mean at the call site. Without that, slick's
    /// `class DriverDataSource(…, classLoader: ClassLoader =
    /// ClassLoaderUtil.defaultClassLoader)` -- written under
    /// `import slick.util.ClassLoaderUtil` -- was `not found: value
    /// ClassLoaderUtil` in every file that called it without that import.
    /// The result is marked `NodeId::PRETYPED_DEFAULT` so the argument list it
    /// is spliced into does not type it a second time in the wrong scope.
    ///
    /// A default whose scope was never recorded (a parameter read from a jar,
    /// where the pickle's getter is the intended route) is typed in the
    /// current scope, as before.
    pub(crate) fn type_default_rhs_here(&mut self, param: SymbolId, rhs: &mut Tree, pty: &Type) {
        let ctx = self.default_scopes.get(&param).map(|d| {
            (
                d.owner,
                d.this_class,
                d.file_index,
                std::rc::Rc::clone(&d.scopes),
            )
        });
        let Some((owner, this_class, file_index, scopes)) = ctx else {
            self.type_default_rhs_in_scope(rhs, pty);
            return;
        };
        let saved_scopes = self.swap_in_scopes(Some(&scopes), owner);
        let saved_owner = std::mem::replace(&mut self.st.owner, owner);
        let saved_this = std::mem::replace(&mut self.st.this_class, this_class);
        let saved_file = std::mem::replace(&mut self.file_index, file_index);
        self.type_default_rhs_in_scope(rhs, pty);
        self.file_index = saved_file;
        self.st.this_class = saved_this;
        self.st.owner = saved_owner;
        self.swap_back_scopes(saved_scopes);
        rhs.id = NodeId::PRETYPED_DEFAULT;
    }

    /// The typing half of `type_default_rhs_here`, in whatever scope is
    /// current. The class's own type parameters are not bound in that scope,
    /// and `class C[A](val l: List[A] = List.empty[A])` reported
    /// `found: List[A] required: List[A]` -- an unresolved *name* against the
    /// class's `A`. Bind the names the parameter's type is written in, so the
    /// two sides agree; a type argument is erased by the time it reaches
    /// codegen.
    fn type_default_rhs_in_scope(&mut self, rhs: &mut Tree, pty: &Type) {
        let mut tps: Vec<SymbolId> = Vec::new();
        collect_tparams(pty, &mut tps);
        tps.retain(|&tp| self.st.lookup(&self.st.get(tp).name).is_empty());
        if !tps.is_empty() {
            self.st.push_scope();
            for tp in &tps {
                let n = self.st.get(*tp).name.clone();
                self.st.enter_in_current(&n, *tp);
            }
        }
        self.type_expr(rhs, pty);
        if !pty.is_no_type() {
            self.adapt(rhs, pty);
        }
        if !tps.is_empty() {
            self.st.pop_scope();
        }
    }

    /// The type parameters a `name$default$n` body may name: the method's own,
    /// plus every one its parameter types mention -- which for a primary
    /// constructor is the enclosing class's. A name already bound by the
    /// method's own list is not rebound.
    fn tparams_in_scope_for_default(
        &self,
        tp_ids: &[SymbolId],
        ret: &Type,
        preceding: &[Type],
    ) -> Vec<SymbolId> {
        let mut used: Vec<SymbolId> = Vec::new();
        collect_tparams(ret, &mut used);
        for p in preceding {
            collect_tparams(p, &mut used);
        }
        let mut out = tp_ids.to_vec();
        for u in used {
            if out.contains(&u) {
                continue;
            }
            let name = self.st.get(u).name.clone();
            if out.iter().any(|&o| self.st.get(o).name == name) {
                continue;
            }
            out.push(u);
        }
        out
    }

    /// Type a `name$default$n` body, in a scope holding the method's type
    /// parameters and the parameters that precede the defaulted one.
    pub(crate) fn type_default_getter_rhs(
        &mut self,
        param: SymbolId,
        getter: SymbolId,
        ret: &Type,
        tparams: &[SymbolId],
        preceding: &[SymbolId],
    ) {
        let Some(rhs) = self.typed_default_body(param, ret, tparams, preceding) else {
            return;
        };
        self.st.get_mut(param).default_rhs = Some(rhs.clone());
        self.st.get_mut(getter).default_rhs = Some(rhs);
    }

    /// Type the stored body of `param`'s default against `ret`, in a scope
    /// holding `tparams` and `preceding`. The caller decides where the typed
    /// tree is stored: an ordinary method's getter writes it back onto the
    /// parameter as well, a constructor's (`crate::ctor_defaults`) does not,
    /// because the parameter's untyped tree is still what a call site splices.
    pub(crate) fn typed_default_body(
        &mut self,
        param: SymbolId,
        ret: &Type,
        tparams: &[SymbolId],
        preceding: &[SymbolId],
    ) -> Option<Tree> {
        let mut rhs = self.st.get(param).default_rhs.clone()?;
        // A repeated parameter's default is a *value*, not an argument list:
        // `case class C(xs: T*)` gives `copy(xs: T* = this.xs)`, and `this.xs`
        // is the `Seq[T]` the field holds. Checking it against `T*` reported a
        // mismatch on a tree nsc never even writes down.
        let seq_ret;
        let ret = match ret {
            Type::Repeated(elem) => {
                seq_ret = self.seq_of(elem).unwrap_or_else(|| ret.clone());
                &seq_ret
            }
            _ => ret,
        };
        self.st.push_scope();
        for tp in tparams {
            let n = self.st.get(*tp).name.clone();
            self.st.enter_in_current(&n, *tp);
        }
        for p in preceding {
            let n = self.st.get(*p).name.clone();
            self.st.enter_in_current(&n, *p);
        }
        self.type_expr(&mut rhs, ret);
        if !ret.is_no_type() {
            self.adapt(&mut rhs, ret);
        }
        self.st.pop_scope();
        Some(rhs)
    }

    pub(crate) fn type_def_body(&mut self, tree: &mut Tree) {
        let saved = self.macro_enter_owner(tree.sym);
        self.type_def_body_with_macro_owner(tree);
        self.macro_lexical_owner = saved;
    }

    fn type_def_body_with_macro_owner(&mut self, tree: &mut Tree) {
        let infer_result = matches!(&tree.kind, TreeKind::DefDef { tpt, .. } if tpt.is_empty());
        let is_ctor = match &tree.kind {
            TreeKind::DefDef { name, .. } => name == "<init>",
            _ => return,
        };
        // Parent instantiations may have been provisional during the signature
        // pass. Refresh the expected result before checking the body, without
        // treating it as the final inferred result.
        if infer_result
            && !is_ctor
            && !tree.sym.is_none()
            && !self.block_local_defs.contains(&(self.file_index, tree.id))
        {
            let method = self.st.get(tree.sym).clone();
            if let Type::Method { paramss, .. } = &method.ty {
                if let Some(expected) = self.overridden_ret_type(
                    method.owner,
                    &method.name,
                    paramss,
                    &method.tparams,
                    &method.params,
                    !method.flags.contains(Flags::OVERRIDE),
                ) {
                    if !expected.is_error() && !expected.is_no_type() {
                        if let Type::Method { ret, .. } = &mut tree.ty {
                            **ret = expected;
                        }
                        self.st.get_mut(tree.sym).ty = tree.ty.clone();
                    }
                }
            }
        }
        {
            let (vparamss, rhs, ret_pt) = match &mut tree.kind {
                TreeKind::DefDef { vparamss, rhs, .. } => {
                    let ret = match &tree.ty {
                        Type::Method { ret, .. } => (**ret).clone(),
                        _ => Type::NoType,
                    };
                    (vparamss, rhs, ret)
                }
                _ => return,
            };
            if rhs.is_empty() {
                self.drop_lazy_sig(tree.sym);
                self.check_stored_annotations(tree);
                return;
            }
            if matches!(rhs.kind, TreeKind::MacroRhs { .. }) {
                // A macro def has no body to type. Record the binding to the
                // implementation instead; the reference is resolved in the
                // enclosing scope, so no parameter scope is pushed.
                self.type_macro_def(tree);
                self.check_stored_annotations(tree);
                return;
            }
            // nsc locks a method while its result type is being inferred, so a
            // definition completed from this body that refers back reports
            // `recursive method f needs result type` at that reference.
            let locked = (ret_pt.is_no_type() || infer_result) && self.lock_lazy_sig(tree.sym);
            self.st.push_scope();
            let saved_owner = self.st.owner;
            let saved_ret = self.return_meth;
            if !tree.sym.is_none() {
                self.return_meth = Some(tree.sym);
                self.st.owner = tree.sym;
                for tp in self.st.get(tree.sym).tparams.clone() {
                    let n = self.st.get(tp).name.clone();
                    self.st.enter_in_current(&n, tp);
                }
            }
            for clause in vparamss.iter() {
                for p in clause {
                    if !p.sym.is_none() {
                        self.st.enter_in_current(p.name().unwrap_or("?"), p.sym);
                    }
                }
            }
            // `typing_call_args` says "this expression is an argument whose
            // parameter has not been picked yet", and it is what makes
            // `adapt_implicit_apply` leave a residual implicit clause standing
            // for the parameter to settle. A method *body* is never that, but
            // the flag is the typer's, not the expression's, and a body typed
            // from inside an argument -- a lazy signature completion above all
            // -- inherited it. slick's
            // `final def buildForeignKeys(builders) = …map(…).flatten` is
            // completed from `m.Table(…, buildForeignKeys(builders), …)`, so
            // `flatten`'s `(A => IterableOnce[B])Seq[B]` was never filled and
            // the method's *inferred result type* became that method type
            // (`jdbc/JdbcModelBuilder.scala:159`). The same definition written
            // above its use compiled fine, which is what gives the flag away.
            let saved_call_args = std::mem::take(&mut self.typing_call_args);
            self.type_expr(rhs, &ret_pt);
            if ret_pt.is_no_type() && !self.reject_unapplied_implicit_clause(rhs) {
                self.adapt_method_value(rhs);
            }
            self.typing_call_args = saved_call_args;
            self.warn_trivial_self_reference(tree.sym, rhs);
            if !ret_pt.is_no_type() {
                self.adapt(rhs, &ret_pt);
            }
            if infer_result && !is_ctor {
                // An inherited result is an expectation, not a written annotation.
                // Keep a narrower inferred result after any required adaptation.
                // SIP-23: do not infer singleton/constant types.
                let inferred = if self
                    .source_features
                    .contains(crate::source_features::SourceFeature::InferOverride)
                    && self.macro_depth == 0
                    && !ret_pt.is_no_type()
                    && !ret_pt.is_error()
                {
                    ret_pt.clone()
                } else {
                    // Close what the body left undetermined, as for a `val`
                    // (`close_leaked_undet`): `def d = inv(fail())` is an
                    // `Inv[Nothing]`.
                    let closed = self.close_leaked_undet(&rhs.ty.widen_constant());
                    self.widen_inferred_singleton(closed)
                };
                if let Type::Method { ret, .. } = &mut tree.ty {
                    **ret = inferred.clone();
                }
                if !tree.sym.is_none() {
                    self.st.get_mut(tree.sym).ty = tree.ty.clone();
                }
            }
            self.st.owner = saved_owner;
            self.return_meth = saved_ret;
            self.st.pop_scope();
            self.unlock_lazy_sig(locked);
        }
        if is_ctor {
            self.check_aux_ctor(tree);
        }
        self.check_stored_annotations(tree);
    }

    pub(crate) fn in_aux_ctor(&self) -> bool {
        self.return_meth
            .map(|id| self.st.get(id).name == "<init>")
            .unwrap_or(false)
    }

    /// SLS 5.3.3: a trait's parents are only *constraints*. `trait T extends C`
    /// records that every class mixing `T` in must be a `C`, but `T` itself
    /// never runs `C`'s constructor -- the concrete class does. So the parent
    /// list of a trait takes no argument list, is not filled with implicits or
    /// defaults, and is not resolved against `C`'s constructor overloads.
    fn in_trait_parents(&self) -> bool {
        match self.parent_ctx {
            Some((owner, _)) if !owner.is_none() => {
                let f = self.st.get(owner).flags;
                f.contains(Flags::TRAIT) || f.contains(Flags::INTERFACE)
            }
            _ => false,
        }
    }

    pub(crate) fn type_parent(&mut self, tree: &mut Tree) {
        // `extends A(1)(2)` is nested Applies; the type is under all of them.
        fn ctor_head(tree: &mut Tree) -> Option<Tree> {
            let mut cur = tree;
            while matches!(&cur.kind, TreeKind::Apply { .. }) {
                let TreeKind::Apply { fun, .. } = &mut cur.kind else {
                    unreachable!()
                };
                cur = fun;
            }
            if matches!(&cur.kind, TreeKind::Empty) {
                return None;
            }
            Some(std::mem::replace(cur, Tree::dummy(TreeKind::Empty)))
        }
        if matches!(&tree.kind, TreeKind::Apply { .. }) {
            if self.in_trait_parents() {
                // scalac 2.13.16 points `parents of traits may not have
                // parameters` at the parent's own name, not at the argument
                // list. Then drop the arguments so the rest of the pass sees a
                // plain parent type and does not cascade.
                match ctor_head(tree) {
                    Some(head) => {
                        let span = head.span;
                        *tree = head;
                        self.error(
                            span,
                            "parents of traits may not have parameters".to_string(),
                        );
                    }
                    None => return,
                }
            } else {
                self.type_parent_ctor_app(tree);
                return;
            }
        }
        let pty = {
            let parent_tree: &Tree = tree;
            self.with_strict_type_names(|s| s.tree_to_type(parent_tree))
        };
        tree.ty = pty;
        self.check_proper_type(&tree.ty, tree.span);
        if let Some(id) = self.st.class_sym_of(&tree.ty) {
            tree.sym = id;
            // `new p.C {}` / `extends p.C` with no argument list, `C` a class
            // or trait nested in a class: `p` is the enclosing instance the
            // new class has to hold -- for a trait, the value its
            // `B$C$$$outer()` returns. The argument-list form types the
            // prefix as an expression (`qualify_parent_type_prefix`); this
            // one left `p` untyped, so nothing captured it and the backend
            // had no instance to hand over (`AbstractMethodError` on
            // `B$C$$$outer()`, run/t4300).
            let owner = self.st.get(id).owner;
            if !self.sigs_only && !owner.is_none() && self.st.get(owner).kind == SymKind::Class {
                type_parent_path_prefix(self, tree);
            }
            if !self.st.get(id).flags.contains(Flags::TRAIT)
                && !self.st.get(id).flags.contains(Flags::INTERFACE)
                && !self.in_trait_parents()
            {
                // `class ConstColumn[T : TT] extends TypedRep[T]` writes no
                // argument list, but `TypedRep`'s only clause is implicit:
                // on the JVM the super call still has to pass it. Give the
                // parent an explicit (empty) application and let the normal
                // call-site machinery fill it, exactly as `new TypedRep[T]`
                // would -- otherwise codegen emits `TypedRep.<init>()`, which
                // type-checks here and fails with `NoSuchMethodError` at run
                // time.
                if !self.sigs_only {
                    self.supply_binary_ctors(id);
                }
                if !self.sigs_only && self.parent_ctor_is_fillable(id) {
                    let head = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
                    *tree = Tree {
                        id: head.id,
                        span: head.span,
                        kind: TreeKind::Apply {
                            fun: Box::new(head),
                            args: Vec::new(),
                        },
                        ty: Type::NoType,
                        sym: SymbolId::NONE,
                        postfix: false,
                        scala_ref: false,
                        stable_pat: false,
                        byname_thunk: false,
                        byname_type_marker: false,
                    };
                    self.type_parent_ctor_app(tree);
                    return;
                }
                let targs: Vec<Type> = match &tree.ty {
                    Type::Class { args, .. } => args.clone(),
                    _ => Vec::new(),
                };
                match self.pick_ctor_at(id, &targs, &[], None) {
                    OverloadPick::Found(sym, _, _) => {
                        tree.sym = id;
                        let _ = sym;
                    }
                    OverloadPick::None => {
                        let has_init = self
                            .st
                            .lookup_member(id, "<init>")
                            .iter()
                            .any(|&m| !self.st.get(m).params.is_empty());
                        if has_init {
                            self.error(
                                tree.span,
                                format!(
                                    "no matching overload for constructor {}",
                                    self.st.get(id).name
                                ),
                            );
                        }
                    }
                    OverloadPick::Ambiguous => {}
                }
            }
        }
    }

    /// `class_id`'s own constructor, when it has exactly one. `lookup_member`
    /// also reports the *inherited* `<init>`, which is not an overload of it;
    /// and a class with auxiliary constructors is resolved from the arguments
    /// that are written, never filled from an empty list.
    pub(crate) fn sole_own_ctor(&self, class_id: SymbolId) -> Option<SymbolId> {
        let alts: Vec<SymbolId> = self
            .st
            .lookup_member(class_id, "<init>")
            .into_iter()
            .filter(|&id| {
                self.st.get(id).kind == crate::symbol::SymKind::Method
                    && self.st.get(id).owner == class_id
            })
            .collect();
        match alts[..] {
            [only] => Some(only),
            _ => None,
        }
    }

    /// `extends P` (or `new P`) with no argument list, where `P`'s constructor
    /// nevertheless takes parameters that a call site is allowed to omit:
    /// every parameter is either implicit or has a default.
    pub(crate) fn parent_ctor_is_fillable(&self, class_id: SymbolId) -> bool {
        let Some(only) = self.sole_own_ctor(class_id) else {
            return false;
        };
        let params = self.st.get(only).params.clone();
        (params.is_empty() && self.st.get(class_id).binary_outer_desc.is_some())
            || (!params.is_empty()
                && params.iter().all(|p| {
                    let f = self.st.get(*p).flags;
                    f.contains(Flags::IMPLICIT) || f.contains(Flags::DEFAULTPARAM)
                }))
    }

    /// Append the constructor arguments a parent clause is allowed to leave
    /// out -- an implicit clause, a defaulted parameter -- so `extends P` and
    /// `extends P(x)` reach codegen with the same flat argument list a
    /// `new P` call site would build.
    ///
    /// Only the body pass does this. The signature pass walks the same tree,
    /// and by then a parent declared in a later unit may still be missing the
    /// evidence parameters its own context bounds add; running once, after
    /// every signature is settled, also keeps the synthesized trees from being
    /// appended twice. `parent_fill_done` is the belt to that braces: an
    /// anonymous class's header can be re-entered from `complete_lazy_sig`.
    fn fill_parent_ctor_args(
        &mut self,
        node: (NodeId, Span),
        span: Span,
        class_id: SymbolId,
        targs: &[Type],
        args: &mut Vec<Tree>,
        ctor: SymbolId,
    ) {
        if self.sigs_only {
            return;
        }
        // The constructor was already selected from the arguments written in
        // the parent clause. Do not require it to be the class's sole
        // constructor: `DriverDataSource(null)` selects the primary
        // constructor while the class also declares a no-argument auxiliary
        // constructor. Using `sole_own_ctor` here silently skipped the
        // primary constructor's defaults and made codegen invoke its full JVM
        // descriptor with only the explicit `null` on the operand stack.
        if ctor.is_none()
            || self.st.get(ctor).kind != crate::symbol::SymKind::Method
            || self.st.get(ctor).owner != class_id
        {
            return;
        }
        self.ensure_external_ctor_defaults(class_id, span);
        let params = self.st.get(ctor).params.clone();
        if args.len() >= params.len() {
            return;
        }
        // Anything else missing is a real "not enough arguments", which the
        // overload report below phrases the way nsc does.
        if !params[args.len()..].iter().all(|p| {
            let f = self.st.get(*p).flags;
            f.contains(Flags::IMPLICIT) || f.contains(Flags::DEFAULTPARAM)
        }) {
            return;
        }
        if !self
            .parent_fill_done
            .insert((self.file_index, node.0, node.1, class_id))
        {
            return;
        }
        // `class TypedRep[T](implicit tpe: TT[T])` states its parameter in its
        // own `T`; `extends TypedRep[String]` must search for `TT[String]`.
        let mut ctor_ty = self.st.get(ctor).ty.clone();
        if !targs.is_empty() {
            ctor_ty = self.st.subst_tparams(class_id, targs, &ctor_ty);
        }
        let param_tys: Vec<Type> = match &ctor_ty {
            Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
            _ => params
                .iter()
                .map(|p| {
                    let t = self.st.get(*p).ty.clone();
                    if targs.is_empty() {
                        t
                    } else {
                        self.st.subst_tparams(class_id, targs, &t)
                    }
                })
                .collect(),
        };
        let ctor_fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "<init>".into(),
            },
            ty: ctor_ty,
            sym: ctor,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
            byname_thunk: false,
            byname_type_marker: false,
        };
        let saved = std::mem::replace(&mut self.parent_ctor_scope, true);
        let _ = self.fill_defaults_and_implicits(span, args, &param_tys, &ctor_fun, &Type::NoType);
        self.parent_ctor_scope = saved;
    }

    /// The class a `new` at the head of an `Apply` chain names, when a plain
    /// name says which it is. Read-only: nothing is typed and nothing is
    /// reported, so it is safe on a tree that is about to be typed properly.
    fn new_head_class(&self, t: &Tree) -> Option<SymbolId> {
        fn head(t: &Tree) -> Option<&Tree> {
            match &t.kind {
                TreeKind::New { tpt } => Some(tpt),
                TreeKind::Apply { fun, .. } => head(fun),
                _ => None,
            }
        }
        fn base_name(t: &Tree) -> Option<&str> {
            match &t.kind {
                TreeKind::Ident { name } => Some(name),
                TreeKind::Select { name, .. } => Some(name),
                TreeKind::AppliedTypeTree { tpt, .. } => base_name(tpt),
                TreeKind::TypeApply { fun, .. } => base_name(fun),
                _ => None,
            }
        }
        let name = base_name(head(t)?)?;
        let mut found = self
            .st
            .lookup_type(name)
            .into_iter()
            .chain(self.st.lookup(name))
            .filter(|&s| self.st.get(s).kind == SymKind::Class);
        let first = found.next()?;
        // Two classes under one name: which constructor this is cannot be
        // settled here, so say nothing rather than guess.
        found.all(|s| s == first).then_some(first)
    }

    /// How many arguments `C`'s constructor can take in all, clauses
    /// flattened, given that the call's first list has `first_len` of them --
    /// `None` when that is not a fixed number (a repeated parameter) or when
    /// there is no constructor to ask.
    ///
    /// The first list picks the alternative: `class Ov(a: Int) { def this(a:
    /// Int, b: Int) = … }` has a two-argument constructor, but `new Ov(1)(2)`
    /// is not it -- the list the user wrote has one argument, so the
    /// one-argument constructor is the one this call is building, and `(2)` is
    /// something else. Alternatives whose first clause is *longer* count only
    /// when none matches exactly, since the extra parameters may be defaults
    /// or implicits the call leaves out.
    fn new_head_ctor_arity(&self, class_id: SymbolId, first_len: usize) -> Option<usize> {
        let alts: Vec<SymbolId> = self
            .st
            .lookup_member(class_id, "<init>")
            .into_iter()
            .filter(|&id| self.st.get(id).kind == SymKind::Method)
            .filter(|&id| self.st.get(id).owner == class_id)
            .collect();
        if alts.is_empty() {
            return None;
        }
        // (first clause length, total length) per alternative.
        let mut shapes: Vec<(usize, usize)> = Vec::new();
        for id in alts {
            let clauses: Vec<Vec<Type>> = match &self.st.get(id).ty {
                Type::Method { paramss, .. } => paramss.clone(),
                _ => vec![self
                    .st
                    .get(id)
                    .params
                    .iter()
                    .map(|p| self.st.get(*p).ty.clone())
                    .collect()],
            };
            if clauses
                .iter()
                .flatten()
                .any(|t| matches!(t, Type::Repeated(_)))
            {
                return None;
            }
            shapes.push((
                clauses.first().map(|c| c.len()).unwrap_or(0),
                clauses.iter().map(|c| c.len()).sum(),
            ));
        }
        let exact: Vec<usize> = shapes
            .iter()
            .filter(|(f, _)| *f == first_len)
            .map(|(_, t)| *t)
            .collect();
        let usable = if exact.is_empty() {
            shapes
                .iter()
                .filter(|(f, _)| *f >= first_len)
                .map(|(_, t)| *t)
                .collect::<Vec<_>>()
        } else {
            exact
        };
        usable.into_iter().max()
    }

    /// How many parameters a constructor's **first** clause declares, read the
    /// same way `new_head_ctor_arity` reads the clause shapes so the two
    /// always agree. `None` when a repeated parameter makes the count
    /// meaningless.
    fn ctor_first_clause_len(&self, id: SymbolId) -> Option<usize> {
        let clauses: Vec<Vec<Type>> = match &self.st.get(id).ty {
            Type::Method { paramss, .. } => paramss.clone(),
            _ => vec![self
                .st
                .get(id)
                .params
                .iter()
                .map(|p| self.st.get(*p).ty.clone())
                .collect()],
        };
        if clauses
            .iter()
            .flatten()
            .any(|t| matches!(t, Type::Repeated(_)))
        {
            return None;
        }
        Some(clauses.first().map(|c| c.len()).unwrap_or(0))
    }

    /// `Apply(Apply(New(C), a), b)` -> `Apply(New(C), a ++ b)`, for as many
    /// lists as `C`'s constructor has room for.
    ///
    /// Only when the head of the whole `Apply` chain is a `New`: every other
    /// nested application is a real call whose result is applied again. And
    /// only while the arguments still fit the constructor: `new Foo(1)(2)` on
    /// a one-parameter `Foo` with an `apply` is `(new Foo(1)).apply(2)` in
    /// nsc, and folding those two lists together would construct a
    /// two-argument `Foo` instead -- silently, where the class has such a
    /// constructor too.
    /// Returns the length of the **first written clause** when clauses were
    /// actually folded together, so the pick can be held to it.
    ///
    /// nsc selects the constructor on the first clause and applies the rest to
    /// what that leaves. Folding first loses that, and the flat list can then
    /// match an alternative the written call never named: `new Three(2)("m")()`
    /// on a class whose primary is `(Int, String)` folds to `(2, "m")`, which
    /// the primary accepts exactly. The arity this folded *by* already came
    /// from the alternatives whose first clause is `first_len` long
    /// (`new_head_ctor_arity`), so holding the pick to the same set is what
    /// keeps the two halves consistent.
    pub(crate) fn flatten_curried_new(&self, tree: &mut Tree) -> Option<Vec<usize>> {
        fn head_is_new(t: &Tree) -> bool {
            match &t.kind {
                TreeKind::New { .. } => true,
                TreeKind::Apply { fun, .. } => head_is_new(fun),
                _ => false,
            }
        }
        fn chain_len(t: &Tree) -> usize {
            match &t.kind {
                TreeKind::Apply { fun, .. } => 1 + chain_len(fun),
                _ => 0,
            }
        }
        // The innermost list is the one the constructor is picked by.
        fn first_list_len(t: &Tree, depth: usize) -> usize {
            match &t.kind {
                TreeKind::Apply { fun, args } => {
                    if depth <= 1 {
                        args.len()
                    } else {
                        first_list_len(fun, depth - 1)
                    }
                }
                _ => 0,
            }
        }
        let lists = chain_len(tree);
        if lists < 2 || !head_is_new(tree) {
            return None;
        }
        // Unknown constructor: fold everything, as this always did.
        // `extends A(1)(2)` takes the same view (`type_parent_ctor_app_in`).
        let first_len = first_list_len(tree, lists);
        let arity = self
            .new_head_class(tree)
            .and_then(|c| self.new_head_ctor_arity(c, first_len))
            .unwrap_or(usize::MAX);
        // Peel the chain apart, innermost list last, keeping each `Apply`'s
        // own node id and span: they are what a re-typed call is recognised
        // by, and a freshly minted id would lose the filled arguments an
        // earlier pass recorded.
        let mut argss: Vec<(NodeId, Span, Vec<Tree>)> = Vec::new();
        let mut head = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        while matches!(head.kind, TreeKind::Apply { .. }) {
            let (id, span) = (head.id, head.span);
            match head.kind {
                TreeKind::Apply { fun, args } => {
                    argss.push((id, span, args));
                    head = *fun;
                }
                _ => unreachable!(),
            }
        }
        argss.reverse();
        // The first list is the constructor's whatever happens; the rest join
        // it only while they still fit.
        let mut take = 1usize;
        let mut n = argss[0].2.len();
        while take < argss.len() && n + argss[take].2.len() <= arity {
            n += argss[take].2.len();
            take += 1;
        }
        let rebuild = |fun: Tree, id: NodeId, span: Span, args: Vec<Tree>| -> Tree {
            let mut t = Tree::dummy(TreeKind::Apply {
                fun: Box::new(fun),
                args,
            });
            t.id = id;
            t.span = span;
            t
        };
        let folded = take > 1;
        let tail = argss.split_off(take);
        let (ctor_id, ctor_span, _) = *argss.last().expect("at least one clause");
        let clause_lengths = argss.iter().map(|(_, _, a)| a.len()).collect();
        let flat: Vec<Tree> = argss.into_iter().flat_map(|(_, _, a)| a).collect();
        let mut out = rebuild(head, ctor_id, ctor_span, flat);
        for (id, span, args) in tail {
            out = rebuild(out, id, span, args);
        }
        *tree = out;
        folded.then_some(clause_lengths)
    }

    fn parent_argument_self_reference(&self, tree: &Tree, module: SymbolId) -> Option<Span> {
        if matches!(tree.kind, TreeKind::Ident { .. } | TreeKind::Select { .. })
            && !tree.sym.is_none()
            && self.st.module_class_of(tree.sym) == module
        {
            return Some(tree.span);
        }
        // Type positions do not evaluate a reference to the module.
        match &tree.kind {
            TreeKind::Typed { expr, .. }
            | TreeKind::TypeApply { fun: expr, .. }
            | TreeKind::ValDef { rhs: expr, .. } => {
                return self.parent_argument_self_reference(expr, module)
            }
            TreeKind::TypeDef { .. }
            | TreeKind::SingletonTypeTree { .. }
            | TreeKind::SelectFromTypeTree { .. }
            | TreeKind::AppliedTypeTree { .. }
            | TreeKind::AnnotatedTypeTree { .. } => return None,
            _ => {}
        }
        let mut found = None;
        crate::erasure::for_each_child(tree, &mut |child| {
            if found.is_none() {
                found = self.parent_argument_self_reference(child, module);
            }
        });
        found
    }

    fn type_parent_ctor_app(&mut self, tree: &mut Tree) {
        // A parent's constructor *arguments* are ordinary expressions, and the
        // signature pass types them before every unit's members have their
        // types. `case class ColumnOrdered[T](column: Rep[T], ord: Ordering)
        // extends Ordered(Vector((column.toNode, ord)))` is compiled with
        // `Rep.scala` later on the command line, so `toNode` was not yet a
        // member there and the tuple came out `(?T1, Ordering)`; the body pass
        // types the very same tree again and gets `(Node, Ordering)`. So the
        // signature pass's complaints about them are dropped, exactly as the
        // header pass's are (`typecheck_units`): anything real is raised again
        // by the pass that runs with every signature in hand.
        let diag_mark = self.sigs_only.then_some(self.diags.len());
        self.type_parent_ctor_app_in(tree);
        if let Some(mark) = diag_mark {
            self.diags.truncate(mark);
        }
    }

    pub(crate) fn type_parent_path_prefix_expr(&mut self, qual: &mut Tree) {
        if qual.ty.is_no_type() && qual.sym.is_none() {
            self.type_expr(qual, &Type::NoType);
        }
    }

    fn qualify_parent_type_prefix(&mut self, head: &mut Tree) {
        match &mut head.kind {
            TreeKind::Select { qual, .. } => {
                self.type_expr(qual, &Type::NoType);
                return;
            }
            TreeKind::AppliedTypeTree { tpt, .. }
            | TreeKind::TypeApply { fun: tpt, .. }
            | TreeKind::AnnotatedTypeTree { tpt, .. } => {
                self.qualify_parent_type_prefix(tpt);
                return;
            }
            _ => {}
        }
        // Preserve the binding's written prefix before dealiasing replaces
        // the head symbol with the underlying class. Different imports may
        // name the same member through distinct enclosing instances.
        if let TreeKind::Ident { name } = &head.kind {
            self.expose_unqualified_type(name, head.span);
            let found = self.st.lookup_type(name);
            let binding = self.st.scopes.iter().rev().find_map(|scope| {
                scope
                    .lookup_ranked(name)
                    .iter()
                    .filter(|b| found.contains(&b.sym))
                    .min_by_key(|b| b.rank)
                    .map(|b| (b.origin, b.sym))
            });
            if let Some((origin, member)) = binding {
                if let Some(mut prefix) = self.parent_import_prefixes.get(&origin).cloned() {
                    self.type_expr(&mut prefix, &Type::NoType);
                    head.kind = TreeKind::Select {
                        qual: Box::new(prefix),
                        name: if let Some(original) =
                            self.parent_import_names.get(&(origin, name.clone()))
                        {
                            original.clone()
                        } else if self.st.get(member).kind == SymKind::Class {
                            name.clone()
                        } else {
                            self.st.get(member).name.clone()
                        },
                    };
                }
            }
        }
    }

    fn type_parent_ctor_app_in(&mut self, tree: &mut Tree) {
        // `extends A(1)(2)` arrives as nested Applies; the constructor takes
        // one flat argument list on the JVM, so flatten the clauses.
        loop {
            let flat = match &mut tree.kind {
                TreeKind::Apply { fun, args } => match &mut fun.kind {
                    TreeKind::Apply {
                        fun: inner_fun,
                        args: inner_args,
                    } => {
                        let mut all = std::mem::take(inner_args);
                        all.append(args);
                        Some((
                            std::mem::replace(inner_fun, Box::new(Tree::dummy(TreeKind::Empty))),
                            all,
                        ))
                    }
                    _ => None,
                },
                _ => return,
            };
            match flat {
                Some((fun, args)) => tree.kind = TreeKind::Apply { fun, args },
                None => break,
            }
        }
        let node = (tree.id, tree.span);
        let fun = match &mut tree.kind {
            TreeKind::Apply { fun, .. } => fun,
            _ => return,
        };
        self.qualify_parent_type_prefix(fun);
        // The parent's prefix (`prefix.rs`) rides on the head tree's type;
        // everything below reads the class, whose constructors and type
        // arguments it is asking about.
        let full_ty = {
            let head: &Tree = fun;
            self.with_strict_type_names(|s| s.tree_to_type(head))
        };
        let parent_pre = crate::prefix::view_prefix(&full_ty).cloned();
        let class_ty = crate::prefix::parent_form(&full_ty);
        fun.ty = crate::prefix::with_prefix_opt(class_ty.clone(), parent_pre.as_ref());
        let class_id = self.st.class_sym_of(&class_ty).unwrap_or(SymbolId::NONE);
        if !class_id.is_none() {
            fun.sym = class_id;
        }
        let hidden = self.hide_template_for_parent_args();
        self.type_parent_ctor_args(node, tree, class_ty, class_id);
        if let Some((index, original)) = hidden {
            self.st.scopes[index] = original;
        }
    }

    /// The template members a parent constructor's arguments must not see.
    ///
    /// nsc types those arguments in the constructor's context, which is
    /// *outside* the template: `class D extends P(a) with A { def a = 1 }`
    /// finds the enclosing `a`, or reports `not found: value a` when there is
    /// none -- never `D`'s own, which would be a call on an instance whose
    /// superclass constructor has not run (a `VerifyError` on
    /// `uninitializedThis` as we emitted it). The template's type parameters
    /// and primary constructor parameters stay visible. Returns the scope to
    /// put back.
    pub(crate) fn hide_template_for_parent_args(
        &mut self,
    ) -> Option<(usize, crate::symbol::Scope)> {
        let (template, mut keep) = self.parent_arg_scope.clone()?;
        let index = self
            .st
            .scopes
            .iter()
            .rposition(|scope| scope.template_owner == Some(template))?;
        // Early definitions (`extends { val a = 3 } with P(a)`) are locals of
        // the constructor ahead of the super call (see `crate::presuper`), so
        // the parent's arguments do see them.
        for m in self.st.get(template).members.clone() {
            let s = self.st.get(m);
            if s.kind == SymKind::Term && s.flags.contains(Flags::PRESUPER) && !keep.contains(&m) {
                keep.push(m);
            }
        }
        let original = std::mem::take(&mut self.st.scopes[index]);
        for id in keep {
            let name = self.st.get(id).name.clone();
            self.st.scopes[index].enter(&name, id);
        }
        Some((index, original))
    }

    fn type_parent_ctor_args(
        &mut self,
        node: (NodeId, Span),
        tree: &mut Tree,
        class_ty: Type,
        class_id: SymbolId,
    ) {
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        // `extends Base(_name = "…", statements = …)` (slick's
        // `MultiInsertAction`). A parent constructor takes named arguments
        // like any other, and for the same reason as `new C(b = 2, a = 1)`
        // they have to be placed *before* the constructor overload is picked,
        // since the pick is driven by the argument types. Without this the
        // `name = value` pairs were typed as assignments to variables that do
        // not exist -- `not found: value _name`, `not found: value statements`
        // -- and the two `Unit`s they left behind produced a third error, `no
        // matching overload for constructor Base with arguments (Unit, Unit)`.
        if Self::has_named_arg(args) {
            let cid = (!class_id.is_none()).then_some(class_id);
            let mut placed = args.clone();
            if self.reorder_named_ctor_args(&mut placed, cid, fun, None) {
                *args = placed;
            } else {
                // Leave the `name = value` arguments in the tree. Every parent
                // constructor is typed twice, and the signature pass throws its
                // diagnostics away (`type_parent_ctor_app`); consuming the
                // named arguments there would hand the body pass -- the pass
                // whose diagnostics are kept -- a positional argument list with
                // nothing left to report but a misleading `no matching
                // overload`. Typing them here would only add the
                // `not found: value <name>` cascade this whole branch exists
                // to remove.
                tree.ty = Type::Error;
                return;
            }
        }
        // Explicit parent type arguments can settle a formal before the
        // argument is typed, including polymorphic evidence expressions.
        self.supply_binary_ctors(class_id);
        let written_targs = match crate::prefix::strip_view(&class_ty) {
            Type::Class { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let raw_tparams = self.st.get(class_id).tparams.clone();
        // `extends NumericOps(lhs)` inside `Integral[T]`: the parent is an
        // inner class of a base trait, read through the enclosing `this`.
        let outer_prefix = self.ctor_outer_prefix(class_id, fun);
        let prototypes: Vec<Vec<Type>> = self
            .st
            .lookup_member(class_id, "<init>")
            .into_iter()
            .filter(|&id| self.st.get(id).owner == class_id)
            .filter_map(|id| {
                let ty = self
                    .st
                    .subst_tparams(class_id, &written_targs, &self.st.get(id).ty);
                let ty = match &outer_prefix {
                    Some(p) => self.st.subst_as_seen_from(p, &ty),
                    None => ty,
                };
                match ty {
                    Type::Method { paramss, .. } => Some(paramss.into_iter().flatten().collect()),
                    _ => None,
                }
            })
            .collect();
        for (i, a) in args.iter_mut().enumerate() {
            let prototype = prototypes
                .first()
                .and_then(|ps| param_at(ps, i))
                .filter(|p| {
                    !self.sigs_only
                        && !mentions_tparam(p, &raw_tparams)
                        && prototypes.iter().all(|ps| param_at(ps, i) == Some(*p))
                })
                .cloned()
                .unwrap_or(Type::NoType);
            let prototype = match prototype {
                // An earlier parent signature pass may already have wrapped
                // this argument in a Function0 thunk. Its type is checked by
                // the selected by-name formal below, not as the raw value.
                Type::ByName(_) if a.byname_thunk => Type::NoType,
                Type::ByName(t) | Type::Repeated(t) => *t,
                t => t,
            };
            // An argument this pass synthesized on an earlier walk of the same
            // parent (a filled implicit or default) is already bound to its
            // symbol. Re-typing it would resolve the name again, in a scope
            // where the evidence parameter it refers to is no longer entered.
            if a.byname_thunk || (a.id == NodeId(0) && !a.ty.is_no_type()) {
                continue;
            }
            if let TreeKind::Function { vparams, .. } = &a.kind {
                if !is_annotated_lambda(a) && prototype.is_no_type() {
                    // As for `new Base(f)`, an unannotated lambda needs the
                    // selected constructor's expected parameter type.
                    a.ty = Type::Function {
                        params: vec![Type::NoType; vparams.len()],
                        ret: Box::new(Type::NoType),
                    };
                    continue;
                }
            }
            self.type_expr(a, &prototype);
            if !prototype.is_no_type() {
                self.adapt(a, &prototype);
            }
        }
        tree.ty = crate::prefix::with_prefix_opt(class_ty.clone(), parent_pre.as_ref());
        if class_id.is_none() {
            return;
        }
        let arg_tys: Vec<Type> = args.iter().map(Tree::argument_type).collect();
        // `class Derived[T](s: Seqn[T]) extends Base(s)` writes no type
        // arguments for `Base`, so nsc infers them from the constructor
        // arguments, exactly as `new Base(s)` would. Without that the
        // parameter stays `Seqn[Base.this.T]`, and both sides of the check
        // print `Seqn[T]` while neither is the other. The inferred arguments
        // become the recorded parent too, so `Derived[X] <: Base[X]` holds.
        let class_ty = self.infer_parent_targs(class_id, &class_ty, &arg_tys);
        fun.ty = crate::prefix::with_prefix_opt(class_ty.clone(), parent_pre.as_ref());
        tree.ty = fun.ty.clone();
        let targs: Vec<Type> = match &class_ty {
            Type::Class { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        // A separately compiled Scala parent may carry its default getter only
        // on the companion (`-Xno-forwarders`, and nested classes).  Mark its
        // constructor parameters before overload selection so an omitted
        // argument is considered applicable and the getter is then type
        // checked through the normal default-argument path.
        self.ensure_external_ctor_defaults(class_id, tree.span);
        self.supply_binary_ctors(class_id);
        let saved_prefix = std::mem::replace(&mut self.ctor_prefix, outer_prefix);
        let picked = self.pick_ctor_at(class_id, &targs, &arg_tys, None);
        self.ctor_prefix = saved_prefix;
        match picked {
            OverloadPick::Found(sym, param_tys, _) => {
                // `class Sub[T](y: T) extends Base[T](y)`: the constructor's
                // parameters are stated in `Base`'s own `T`, and
                // `pick_ctor_at` has already read them at the type arguments
                // the `extends` clause wrote -- otherwise both sides of the
                // mismatch print `T` and neither one is the other. Doing it
                // again here is not a no-op when an argument mentions the
                // parameter it replaces.
                for (i, a) in args.iter_mut().enumerate() {
                    if let Some(p) = param_at(&param_tys, i) {
                        if !p.is_no_type() {
                            if matches!(a.kind, TreeKind::Function { .. })
                                && !is_annotated_lambda(a)
                            {
                                self.type_expr(a, p);
                            } else {
                                self.adapt(a, p);
                            }
                        }
                        let owner = self.st.this_class;
                        if !owner.is_none() && self.st.get(owner).kind == SymKind::ModuleClass {
                            if let Some(span) = self.parent_argument_self_reference(a, owner) {
                                self.error(span, "super constructor cannot be passed a self reference unless parameter is declared by-name");
                            }
                        }
                    }
                }
                tree.sym = sym;
                // Only now, with the constructor settled from what the source
                // wrote: the omitted implicit / defaulted arguments are
                // appended, and the overload is never re-resolved against
                // them. A synthesized argument that failed to conform would
                // otherwise turn one honest "could not find implicit" into a
                // second, misleading "no matching overload".
                self.fill_parent_ctor_args(node, tree.span, class_id, &targs, args, sym);
                fn qualifier(t: &Tree) -> Option<&Tree> {
                    match &t.kind {
                        TreeKind::Select { qual, .. } => Some(qual),
                        TreeKind::AppliedTypeTree { tpt, .. }
                        | TreeKind::TypeApply { fun: tpt, .. } => qualifier(tpt),
                        _ => None,
                    }
                }
                let mut parent_path = qualifier(fun).map(|q| q.ty.clone());
                if let Some(q) = qualifier(fun) {
                    if let TreeKind::Select { qual, name } = &q.kind {
                        if let (Some(receiver), Some(member)) =
                            (self.st.class_sym_of(&qual.ty), self.st.class_sym_of(&q.ty))
                        {
                            if let Some(owner) = self.pickle.member_module_owner(
                                &mut self.st,
                                &mut self.binary,
                                receiver,
                                name,
                            ) {
                                if self.st.get(member).owner == owner {
                                    parent_path = Some(Type::SingleType {
                                        prefix: Box::new(qual.ty.clone()),
                                        sym: member,
                                    });
                                }
                            }
                        }
                    }
                }
                if let Some(Type::SingleType {
                    prefix,
                    sym: member,
                }) = parent_path.as_ref()
                {
                    if let Type::ModuleRef(module) = &**prefix {
                        self.ensure_classfile_members_loaded(*module, "", tree.span);
                        let member_class = self.st.module_class_of(*member);
                        let owner = self.st.get(member_class).owner;
                        let outer = self
                            .st
                            .get(class_id)
                            .binary_outer_desc
                            .as_deref()
                            .and_then(|d| d.strip_prefix('L')?.strip_suffix(';'))
                            .and_then(|jvm| self.st.find_class_by_jvm(jvm));
                        if let Some(outer) = outer {
                            self.pickle
                                .ensure_parents(&mut self.st, &mut self.binary, owner);
                            if self.st.is_sub_type(
                                &Type::Class {
                                    sym: owner,
                                    args: vec![],
                                },
                                &Type::Class {
                                    sym: outer,
                                    args: vec![],
                                },
                            ) && self.st.is_sub_type(
                                &Type::ModuleRef(*module),
                                &Type::Class {
                                    sym: outer,
                                    args: vec![],
                                },
                            ) {
                                self.st
                                    .parent_outer_modules
                                    .insert(self.st.this_class, *module);
                            }
                        }
                    }
                }
            }
            OverloadPick::Ambiguous => {
                self.error(tree.span, "ambiguous overload for constructor");
            }
            OverloadPick::None => {
                self.error(
                    tree.span,
                    format!(
                        "no matching overload for constructor {} with arguments ({})",
                        self.st.get(class_id).name,
                        arg_tys
                            .iter()
                            .map(|t| self.st.display_type(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
        }
    }

    /// nsc's `parentTypes`: an `extends` clause that names a parameterized
    /// class without type arguments gets them from the constructor arguments,
    /// the same inference `new Base(s)` runs. A parameter no argument mentions
    /// is left alone -- writing nothing there keeps today's behaviour rather
    /// than inventing an `Any`.
    fn infer_parent_targs(&self, class_id: SymbolId, class_ty: &Type, arg_tys: &[Type]) -> Type {
        if !matches!(class_ty, Type::Class { args, .. } if args.is_empty()) {
            return class_ty.clone();
        }
        let tps = self.st.get(class_id).tparams.clone();
        if tps.is_empty() || arg_tys.iter().any(|t| t.is_no_type() || t.is_error()) {
            return class_ty.clone();
        }
        let mut solutions = Vec::new();
        for ctor in self.st.lookup_member(class_id, "<init>") {
            if self.st.get(ctor).owner != class_id {
                continue;
            }
            let Type::Method { paramss, .. } = &self.st.get(ctor).ty else {
                continue;
            };
            let param_tys: Vec<Type> = paramss.iter().flatten().cloned().collect();
            let inferred: Option<Vec<Type>> = tps
                .iter()
                .map(|tp| {
                    self.unify_tparam_all(*tp, &param_tys, arg_tys).filter(|t| {
                        !t.is_no_type() && !t.is_error() && !type_mentions_tparam(t, *tp)
                    })
                })
                .collect();
            let Some(inferred) = inferred else {
                continue;
            };
            if matches!(self.pick_ctor_at(class_id, &inferred, arg_tys, None),
                OverloadPick::Found(picked, _, _) if picked == ctor)
                && !solutions.contains(&inferred)
            {
                solutions.push(inferred);
            }
        }
        if solutions.len() == 1 {
            Type::Class {
                sym: class_id,
                args: solutions.pop().unwrap(),
            }
        } else {
            class_ty.clone()
        }
    }

    fn pick_ctor(
        &self,
        class_id: SymbolId,
        arg_tys: &[Type],
        skip: Option<SymbolId>,
    ) -> OverloadPick {
        self.pick_ctor_at(class_id, &[], arg_tys, skip)
    }

    /// `targs` are the type arguments the constructor is applied at
    /// (`extends TypedRep[T](tt)`): a parameter declared as `TT[T]` in terms of
    /// the *parent's* type parameter has to be read as `TT[T]` of the subclass
    /// before the arguments are matched against it.
    pub(crate) fn pick_ctor_at(
        &self,
        class_id: SymbolId,
        targs: &[Type],
        arg_tys: &[Type],
        skip: Option<SymbolId>,
    ) -> OverloadPick {
        self.pick_ctor_at_clause(class_id, targs, arg_tys, skip, None)
    }

    /// The same, restricted to the alternatives whose **first parameter
    /// clause** is `first_clause_len` long.
    ///
    /// Only a curried `new` passes one: `flatten_curried_new` folded its
    /// clauses into a flat argument list, and without this the flat list can
    /// match an alternative whose first clause the written call never fitted.
    /// The restriction is dropped when it would leave nothing, so a class
    /// whose constructor this cannot describe is picked exactly as before.
    pub(crate) fn pick_ctor_at_clause(
        &self,
        class_id: SymbolId,
        targs: &[Type],
        arg_tys: &[Type],
        skip: Option<SymbolId>,
        first_clause_len: Option<usize>,
    ) -> OverloadPick {
        if class_id.is_none() {
            return OverloadPick::None;
        }
        let own_ctors: Vec<SymbolId> = self
            .st
            .lookup_member(class_id, "<init>")
            .into_iter()
            .filter(|&id| Some(id) != skip)
            .filter(|&id| self.st.get(id).kind == crate::symbol::SymKind::Method)
            // Constructors are not inherited (see `sole_own_ctor`): a parent's
            // `<init>` that `lookup_member` walks into is not an alternative
            // of this class's own.
            .filter(|&id| self.st.get(id).owner == class_id)
            .collect();
        // A separately compiled curried constructor can be supplied twice:
        // its pickle exposes the first clause as a source-shaped constructor
        // without a JVM descriptor, while the classfile/pickle repair supplies
        // the flattened descriptor.  The descriptor-bearing symbol is the
        // callable constructor; retaining the descriptorless partial symbol
        // makes an omitted default look like an ambiguous overload.
        let has_linked_ctor = own_ctors
            .iter()
            .any(|&id| !self.st.get(id).jvm_name.is_empty());
        let alts: Vec<SymbolId> = own_ctors
            .into_iter()
            .filter(|&id| !has_linked_ctor || !self.st.get(id).jvm_name.is_empty())
            .collect();
        if alts.is_empty() {
            return OverloadPick::None;
        }
        // A curried `new` selected its constructor on the first clause before
        // `flatten_curried_new` folded the rest into it. Keep the pick to that
        // same set, and only while the set is non-empty.
        let alts: Vec<SymbolId> = match first_clause_len {
            Some(n) => {
                let kept: Vec<SymbolId> = alts
                    .iter()
                    .copied()
                    .filter(|&id| self.ctor_first_clause_len(id) == Some(n))
                    .collect();
                if kept.is_empty() {
                    alts
                } else {
                    kept
                }
            }
            None => alts,
        };
        // `extends A(1)(2)` and `new A(1)(2)` pass one flat argument list, so a
        // multi-clause constructor is matched against its flattened clauses.
        let flatten = |ty: Type| -> Type {
            let ty = if targs.is_empty() {
                ty
            } else {
                self.st.subst_tparams(class_id, targs, &ty)
            };
            // An inner class's parameters, read through the prefix it is
            // instantiated on (`Typer::ctor_outer_prefix`).
            let ty = match &self.ctor_prefix {
                Some(p) => self.st.subst_as_seen_from(p, &ty),
                None => ty,
            };
            match ty {
                Type::Method { paramss, ret } if paramss.len() > 1 => Type::Method {
                    paramss: vec![paramss.into_iter().flatten().collect()],
                    ret,
                },
                other => other,
            }
        };
        // A constructor the signature pass has not typed yet still knows its
        // parameter *symbols*, so read it from those rather than skipping it.
        let alt_ty = |id: SymbolId| -> Type {
            let ty = self.st.get(id).ty.clone();
            if ty.is_no_type() {
                Type::Method {
                    paramss: vec![self
                        .st
                        .get(id)
                        .params
                        .iter()
                        .map(|p| self.st.get(*p).ty.clone())
                        .collect()],
                    ret: Box::new(Type::Unit),
                }
            } else {
                flatten(ty)
            }
        };
        let fun_sym = alts[0];
        let alt_tys: Vec<Type> = alts.iter().map(|&id| alt_ty(id)).collect();
        let fun_ty = if alts.len() == 1 {
            alt_tys[0].clone()
        } else {
            Type::Overload(alt_tys.clone())
        };
        // `resolve_overload` re-reads a group of two or more alternatives off
        // their symbols, where they are written in the *parent's* type
        // parameters; the arguments are in the subclass's. `Eq0[SortedMapEq.V]`
        // and `Hash0[SortedMapHash.V]` are different symbols, so nothing
        // matched and cats-kernel's `extends SortedMapEq[K, V]()(V)` reported
        // `no matching overload for constructor SortedMapEq` -- but only once
        // the class had a second constructor, since with one alternative the
        // clause `flatten` built at `targs` is used as is. Handing the
        // instantiated alternatives over keeps both paths reading the same
        // types, and makes what comes out of `pick_ctor_at` read at `targs`
        // exactly once.
        let at_targs: Vec<(SymbolId, Type)> = alts.iter().copied().zip(alt_tys).collect();
        match self.resolve_overload_with(&fun_ty, fun_sym, arg_tys, &Type::NoType, Some(&at_targs))
        {
            OverloadPick::Found(sym, _, _) if Some(sym) == skip => OverloadPick::None,
            other => other,
        }
    }

    pub(crate) fn type_ctor_delegation(&mut self, tree: &mut Tree) {
        let tree_id = tree.id;
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        let is_super = matches!(&fun.kind, TreeKind::Super { .. })
            || matches!(&fun.kind, TreeKind::Ident { name } if name == "super");
        self.type_expr(fun, &Type::NoType);
        if is_super {
            self.error(
                tree.span,
                "auxiliary constructor cannot call super(...); the first action must be this(...)",
            );
            for a in args.iter_mut() {
                self.type_expr(a, &Type::NoType);
            }
            tree.ty = Type::Unit;
            return;
        }
        let class_id = self.st.this_class;
        let skip = self.return_meth;
        // `def this() = this(a = 1, initBlank = true)`. A `name = value`
        // argument is a *named argument*, not an assignment, and this path had
        // no idea: the pairs were typed as `Assign` trees, so every one
        // reported `reassignment to val <name>` against the primary
        // constructor's own parameter (which is in scope in an auxiliary
        // constructor's body), and the `Unit`s they left behind produced a
        // further `no matching overload for constructor C with arguments
        // (Unit, Unit)`. `AnyRefMap`'s four auxiliary constructors are eight
        // of those errors on their own. The names have to be placed before the
        // overload is picked, since `pick_ctor` is driven by argument types --
        // the same reason `new C(b = 2, a = 1)` and `extends B(b = 2, a = 1)`
        // place theirs first.
        if Self::has_named_arg(args) {
            let placed = self.reorder_named_ctor_args(args, Some(class_id), fun, skip);
            self.record_named_arg_order(tree_id);
            if !placed {
                for a in args.iter_mut() {
                    self.type_expr(a, &Type::NoType);
                }
                tree.ty = Type::Unit;
                return;
            }
        }
        // A delegation's arguments precede initialization of this instance.
        // Keep lexical outer scopes and this constructor's parameters, but
        // hide template members and imports until the delegation has finished.
        // Scope ownership survives lazy-signature snapshots.
        let template_scope = self
            .st
            .scopes
            .iter()
            .position(|scope| scope.template_owner == Some(class_id));
        let saved_scope = template_scope.map(|index| {
            let original = std::mem::take(&mut self.st.scopes[index]);
            for tp in self.st.get(class_id).tparams.clone() {
                let name = self.st.get(tp).name.clone();
                self.st.scopes[index].enter(&name, tp);
            }
            (index, original)
        });
        let mut outer = self.st.get(class_id).owner;
        while !outer.is_none() && !self.st.get(outer).is_class_like() {
            outer = self.st.get(outer).owner;
        }
        let saved_this = std::mem::replace(&mut self.st.this_class, outer);
        for a in args.iter_mut() {
            self.type_expr(a, &Type::NoType);
        }
        let arg_tys: Vec<Type> = args.iter().map(Tree::argument_type).collect();
        match self.pick_ctor(class_id, &arg_tys, skip) {
            OverloadPick::Found(sym, param_tys, _) => {
                if let Some(cur) = skip {
                    if !self.ctor_precedes(sym, cur) {
                        self.error(
                            tree.span,
                            "called constructor's definition must precede calling constructor's definition",
                        );
                    }
                }
                // `def this() = this("u0")` leaves the defaulted parameters
                // out exactly as a `new` would, and the call has to reach
                // codegen with the flat argument list the constructor's
                // descriptor promises. Without this the emitted `<init>`
                // pushed one argument for a five-parameter `invokespecial`:
                // slick's `DriverDataSource`, whose `def this() = this(null)`
                // stands in front of eight defaults, failed the verifier
                // before its first line ran.
                let ctor_fun = Tree {
                    id: NodeId(0),
                    span: tree.span,
                    kind: TreeKind::Ident {
                        name: "<init>".into(),
                    },
                    ty: self.st.get(sym).ty.clone(),
                    sym,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                    byname_thunk: false,
                    byname_type_marker: false,
                };
                let _ = self.fill_defaults_and_implicits(
                    tree.span,
                    args,
                    &param_tys,
                    &ctor_fun,
                    &Type::NoType,
                );
                for (i, a) in args.iter_mut().enumerate() {
                    if let Some(p) = param_tys.get(i) {
                        if !p.is_no_type() {
                            self.adapt(a, p);
                        }
                    }
                }
                fun.sym = sym;
                tree.sym = sym;
                tree.ty = Type::Unit;
            }
            OverloadPick::Ambiguous => {
                self.error(tree.span, "ambiguous overload for constructor");
                tree.ty = Type::Unit;
            }
            OverloadPick::None => {
                self.error(
                    tree.span,
                    format!(
                        "no matching overload for constructor {} with arguments ({})",
                        self.st.get(class_id).name,
                        arg_tys
                            .iter()
                            .map(|t| self.st.display_type(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
                tree.ty = Type::Unit;
            }
        }
        self.st.this_class = saved_this;
        if let Some((index, scope)) = saved_scope {
            self.st.scopes[index] = scope;
        }
    }

    fn ctor_precedes(&self, called: SymbolId, caller: SymbolId) -> bool {
        if called == caller {
            return false;
        }
        let owner = self.st.get(caller).owner;
        let members = &self.st.get(owner).members;
        let pos = |id: SymbolId| members.iter().position(|&m| m == id);
        match (pos(called), pos(caller)) {
            (Some(a), Some(b)) => a < b,
            _ => true,
        }
    }

    fn check_aux_ctor(&mut self, tree: &Tree) {
        let rhs = match &tree.kind {
            TreeKind::DefDef { rhs, name, .. } if name == "<init>" => rhs,
            _ => return,
        };
        match first_ctor_delegation(rhs) {
            CtorDelegation::This => {}
            CtorDelegation::Super => {
                self.error(
                    rhs.span,
                    "auxiliary constructor cannot call super(...); the first action must be this(...)",
                );
            }
            CtorDelegation::AfterStats => {
                self.error(
                    rhs.span,
                    "constructor invocation must be the first statement in an auxiliary constructor",
                );
            }
            CtorDelegation::Missing => {
                self.error(
                    rhs.span,
                    "auxiliary constructor must start with a call to this(...)",
                );
            }
        }
    }
}

/// Type the term prefix of a parent written as a type path (`p.C`,
/// `p.C[T]`, `p.C @ann`) -- see the call in `Typer::type_parent`. Only a
/// prefix the type pass left without a type or symbol is typed, so a path
/// already attributed is not typed twice.
fn type_parent_path_prefix(t: &mut Typer, head: &mut Tree) {
    match &mut head.kind {
        TreeKind::Select { qual, .. } => t.type_parent_path_prefix_expr(qual),
        TreeKind::AppliedTypeTree { tpt, .. }
        | TreeKind::TypeApply { fun: tpt, .. }
        | TreeKind::AnnotatedTypeTree { tpt, .. } => type_parent_path_prefix(t, tpt),
        _ => {}
    }
}

/// Whether `tree` contains a reference (`Ident` or `Select`) to `sym`.
fn tree_mentions_sym(tree: &Tree, sym: SymbolId) -> bool {
    if matches!(tree.kind, TreeKind::Ident { .. } | TreeKind::Select { .. }) && tree.sym == sym {
        return true;
    }
    let mut found = false;
    crate::erasure::for_each_child(tree, &mut |c| {
        if !found && tree_mentions_sym(c, sym) {
            found = true;
        }
    });
    found
}
