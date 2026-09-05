#![allow(dead_code)]
//! Selecting a member (`qual.name`), deciding whether it may be seen, and
//! the rewrites a selection can turn into.
//!
//! `type_select` finds the member, records the overload group when there is
//! more than one alternative, and applies the access rules -- `private`,
//! `protected`, qualified access boundaries, companions, Java package access.
//! When no member is found the selection may still be rewritten: `Dynamic`,
//! assignment operators such as `+=`, `case class` `copy`, and extension
//! methods reached through an implicit conversion.

use crate::check::*;
// Named explicitly: `scala_rs_parser::ast` exports a different function of
// this name, and both arrive here through a glob. See `check::is_assignment_op`.
use crate::check::is_assignment_op;
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::ast::*;
use scala_rs_span::Span;
use std::collections::HashSet;

impl Typer {
    pub(crate) fn type_select(&mut self, tree: &mut Tree, pt: &Type) {
        if tree.postfix && !self.language_postfix_ops {
            let name = match &tree.kind {
                TreeKind::Select { name, .. } => name.clone(),
                _ => String::new(),
            };
            self.warning(
                tree.span,
                format!(
                    "postfix operator {name} should be enabled by making the implicit value scala.language.postfixOps visible"
                ),
            );
        }
        let (qual, name) = match &mut tree.kind {
            TreeKind::Select { qual, name } => (qual, name.clone()),
            _ => return,
        };
        if qual.ty.is_no_type() {
            // The enclosing application's argument count belongs to *this*
            // selection, not to whatever the qualifier turns out to be.
            let saved_arity = self.callee_arity.take();
            self.type_expr(qual, &Type::NoType);
            self.callee_arity = saved_arity;
            // A *qualifier* is never "an argument still waiting for its
            // alternative": `pack` in `SV(pack.to[Seq], "x")` has to be a
            // value before `to` can be looked up on it, exactly as nsc types
            // a qualifier in EXPRmode and adapts it. `adapt_implicit_apply`
            // bails while `typing_call_args` is set, which is about the
            // *argument* tree, and letting that reach down into a qualifier
            // inside it left slick's `ShapedValue(pack.to[Seq], …)` with
            // `value to is not a member of (Shape[…])Query[R, U, C]`.
            //
            // Retried here rather than by clearing the flag around
            // `type_expr` above: the flag also decides how a *tag* request
            // inside the qualifier is answered, and clearing it wholesale
            // made `weakTypeOf[ExBox[E]]` in `tests/fixtures/ex_impl.scala`
            // pick the tag in scope for `E` instead of composing one
            // (`ExBox[ExRow]` printed as `ExRow`). Only a clause that
            // actually survived is retried.
            if self.implicit_only_result(qual).is_some() {
                let saved = std::mem::replace(&mut self.typing_call_args, false);
                self.adapt_implicit_apply(qual, &Type::NoType);
                self.typing_call_args = saved;
            }
            // …and a clause that could not be filled even then is the missing
            // implicit, not `value to is not a member of (Sh[Int, R])Qy[R]`.
            // `adapt`'s backstop never sees a qualifier: it is typed with no
            // expected type at all.
            if self.reject_unapplied_implicit_clause(qual) {
                tree.ty = Type::Error;
                return;
            }
        }
        if name == "_" {
            self.error(
                tree.span,
                "unimplemented syntax: wildcard import/select in expression",
            );
            tree.ty = Type::Error;
            return;
        }
        // nsc: `x.m` on `x: A` where `A <: T` resolves against `T`.
        // An alias member (`type Scope = Map[K, V]`) is dealiased first, or the
        // receiver's type arguments would be invisible to the substitution below.
        let mut recv_ty = self.st.dealias(&self.st.widen_type_param(&qual.ty));
        // `super.m` (`qual` is a `Super` tree): resolve `m` against the real
        // mixin linearization, not against the one parent `TreeKind::Super`
        // picked independent of `m`'s name and not through that parent's own
        // self-type (`lookup_member_real`/`super_select_member`'s doc comment
        // explains why -- reusing the generic member search below found a
        // self-type-provided member of the very definition being completed
        // and reported a false `recursive method … needs result type`).
        let mut super_found: Option<Vec<SymbolId>> = None;
        // `super.m`'s type is seen from `this.type`, not from the parent named
        // on its own: an abstract type member the parent declares stands for
        // *this* class's implementation of it. Remember which class that is.
        let mut super_this: Option<SymbolId> = None;
        if let TreeKind::Super { qual: sq, mix } = &qual.kind {
            let this_id = if let Some(nm) = sq.clone() {
                self.st
                    .enclosing_class_named(self.st.this_class, &nm)
                    .unwrap_or(self.st.this_class)
            } else {
                self.st.this_class
            };
            if let Some((parent, members)) =
                self.super_select_member(this_id, mix.as_deref(), &name)
            {
                recv_ty = self.super_prefix_type(this_id, parent);
                super_found = Some(members);
                super_this = Some(this_id);
            }
        }
        let refined_term = match &recv_ty {
            Type::Refined { decls, .. } => {
                let from_term = decls.iter().any(|d| {
                    matches!(
                        d,
                        scala_rs_parser::RefineDecl::Def { name: n, .. }
                            | scala_rs_parser::RefineDecl::Val { name: n, .. }
                            if n == &name
                    )
                });
                if from_term {
                    SymbolTable::refine_member_type(decls, &name)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(mty) = refined_term {
            let mty = self.st.expand_in_type(&recv_ty, &mty);
            tree.ty = self.maybe_auto_apply(mty, pt);
            return;
        }
        // String concatenation via any2stringadd: handled at Apply of +
        let mut found = super_found.take().unwrap_or_default();
        if let Type::Refined { parents, .. } = &recv_ty {
            for p in parents {
                if let Some(o) = self.st.class_sym_of(p) {
                    found.extend(self.st.lookup_member(o, &name));
                }
            }
        }
        // nsc: "Static Java members belong to companion objects in Scala; they
        // are not inherited". `b.parseInt("12")` on a `java.lang.Integer` value
        // is an error in scalac, and letting statics through here is not merely
        // lax: `java.lang.Integer.max(int,int)` competed with `RichInt.max` for
        // `1.max(2)` and left the extension search with no winner.
        let instance_receiver = !self.is_type_qualifier(qual);
        if found.is_empty() {
            if let Some(o) = self.st.class_sym_of(&recv_ty) {
                found = self.st.lookup_member(o, &name);
                if instance_receiver {
                    found.retain(|&m| !self.st.get(m).flags.contains(Flags::STATIC));
                }
                if found.is_empty() && matches!(&recv_ty, Type::Class { .. } | Type::ModuleRef(_)) {
                    // `asList(...).size()`: the receiver type is a Java stub until
                    // the classfile is completed. `qual.sym` is the method, not List.
                    // Skip `Type::String` / primitives so StringOps / RichChar views
                    // are not shadowed by `java.lang.String` / `Character` overloads.
                    self.ensure_java_loaded(o, tree.span);
                    found = self.st.lookup_member(o, &name);
                    if instance_receiver {
                        found.retain(|&m| !self.st.get(m).flags.contains(Flags::STATIC));
                    }
                }
            }
        }
        // Module: members of module class
        if found.is_empty() {
            if let Type::ModuleRef(id) = &recv_ty {
                found = self.st.lookup_member(*id, &name);
            }
        }
        // A package written out in an expression:
        // `cats.kernel.instances.int.catsKernelStdOrderForInt`. A package has
        // no type, so every search above looked at `<notype>` and found
        // nothing; what the selection means is a member of the package -- of
        // its `package object`, which the namer folds into the package for a
        // same-run source and which hangs off `p/package$` when it comes from
        // a jar. Only the import path used to know this.
        if found.is_empty() && recv_ty.is_no_type() {
            let pkg = qual.sym;
            let span = tree.span;
            if !pkg.is_none() && self.st.get(pkg).kind == SymKind::Package {
                self.complete_binary_member(pkg, &name, span);
                let mut cand = self.st.lookup_member(pkg, &name);
                let po = self.package_object_of(pkg, span);
                if cand.is_empty() {
                    if let Some(po) = po {
                        self.complete_binary_member(po, &name, span);
                        cand = self.st.lookup_member(po, &name);
                    }
                }
                // A package is not a value: a `val`/`def` reached through one
                // is really a member of its package object, and that module is
                // the receiver the backend has to push. A nested object or
                // class needs no receiver, so it keeps the package prefix.
                if let Some(po) = po {
                    let term = !cand.is_empty()
                        && cand.iter().all(|&m| {
                            matches!(self.st.get(m).kind, SymKind::Method | SymKind::Term)
                        });
                    if term {
                        qual.ty = Type::ModuleRef(po);
                        qual.sym = po;
                        recv_ty = Type::ModuleRef(po);
                    }
                }
                found = cand;
            }
        }
        // A function type is `scala.FunctionN[T1, …, Tn, R]`; `class_sym_of`
        // has no symbol for it, so `f.tupled` / `f.curried` would find nothing.
        if found.is_empty() {
            if let Some(f) = self.function_class_of(&recv_ty) {
                found = self.st.lookup_member(f, &name);
            }
        }
        // Package / term prefix: `scala.reflect.ClassTag` and Java `java.lang.Math`.
        //
        // A stable value's *type* -- not `qual.sym` -- names the real class or
        // module it stands for. A package object's `val Box = tinylib.Box`
        // (cats.effect's `package object effect` does this for `Resource` and
        // `Outcome`) makes `qual.sym` the *val*, whose `jvm_name` is empty;
        // `complete_binary_member` then built candidates like `$Const` from
        // that empty name and never found `Box.Const`, even though `Box.of` --
        // already loaded onto the module class when the jar's own classfile
        // was read -- worked fine. `class_sym_of` unwraps `Type::ModuleRef` /
        // `Type::Class` to the actual class-like symbol regardless of how the
        // term reached it, so try that first and fall back to `qual.sym` for
        // package/Java-static prefixes, which carry no `Type` of their own.
        if found.is_empty() {
            // Scoped to `Type::ModuleRef` specifically, not every
            // `class_sym_of` hit: `complete_binary_member` on a *class*
            // receiver (`Type::Class`, e.g. `Type::String`) calls
            // `ensure_java_loaded` and pulls in the classfile's own members
            // -- for `java.lang.String` that is JDK 11's `lines(): Stream
            // <String>`, which then shadowed 2.13's deprecated `StringOps.
            // lines: Iterator[String]` and the extension search below never
            // got a chance to run. A module reference never has that
            // problem: `complete_binary_member` on a `ModuleClass` owner
            // only tries nested-class/companion candidates, never eagerly
            // loads the receiver's own classfile.
            if let Type::ModuleRef(o) = &recv_ty {
                let o = *o;
                self.complete_binary_member(o, &name, tree.span);
                found = self.st.lookup_member(o, &name);
            }
            // Never a *method*. `qual.sym` on an application is the callee,
            // and a method symbol's member list is its own parameters
            // (`PickleSupply::install` allocates each one owned by the
            // method). `m.staticClass(n).fullName` therefore found
            // `staticClass`'s parameter -- which is called `fullName` -- as
            // if it were a member of the result, and codegen read it as a
            // field whose owner class was the method's erased descriptor:
            // `ClassFormatError: Illegal class name
            // "(Ljava/lang/String;)Lscala/reflect/api/Symbols$ClassSymbolApi;"`
            // at load time, from a compile that reported nothing.
            if found.is_empty()
                && !qual.sym.is_none()
                && self.st.get(qual.sym).kind != SymKind::Method
            {
                self.complete_binary_member(qual.sym, &name, tree.span);
                found = self.st.lookup_member(qual.sym, &name);
            }
        }
        if found.is_empty() && name == "toString" {
            found = self.st.lookup_member(self.st.any_sym, "toString");
        }
        // The library's own pickle, *before* the view search: a member the
        // receiver really has always beats an implicit conversion (SLS 6.26.1
        // applies a view only when the selection does not type-check, and nsc
        // has every member loaded by then). scala-rs loads them on demand, so
        // asking after the view search meant a member that merely had not been
        // read yet lost to one. `1.second + 500.millis` was the case that
        // showed it: `FiniteDuration`'s classfile spells `+` as `$plus`, so
        // lookup missed it, `any2stringadd` claimed the selection, and the
        // call came out as `no matching overload for (String)String with
        // arguments (FiniteDuration)` instead of `FiniteDuration.$plus`.
        // Still gated on nothing having matched, so it can only add members.
        if found.is_empty() {
            found = self.supply_from_pickle(&recv_ty, &name);
        } else {
            self.supply_receiver_override(&recv_ty, &name, &mut found);
        }
        // An abstract type member whose upper bound is a *compound* offers
        // every parent's members, and only the first one had been reachable.
        if found.is_empty() {
            found = self.members_through_compound_bound(&recv_ty, &name);
        }
        // The same name in both namespaces, term side missing. The reflect API
        // writes `type Modifiers >: Null <: ModifiersApi` next to
        // `def Modifiers(flags: FlagSet): Modifiers`, and a jar class's
        // members are read one name at a time -- so once the *type* member was
        // installed (completing `NoMods`, whose result type is `Modifiers`,
        // installs it), the name was no longer missing here, the term
        // overloads were never read, and `u.Modifiers(flags)` selected a
        // `TypeMember` whose value type is `NoType`: "value apply is not a
        // member of <notype>". The mirror image of `expose_unqualified_type`
        // (`docs/macros.md` §7.6), and just as additive: only a *term*-shaped
        // member can win here.
        if !found.is_empty()
            && found
                .iter()
                .all(|&s| self.st.get(s).kind == SymKind::TypeMember)
        {
            let more = self.supply_from_pickle(&recv_ty, &name);
            let terms: Vec<SymbolId> = more
                .into_iter()
                .filter(|&s| {
                    matches!(
                        self.st.get(s).kind,
                        SymKind::Module | SymKind::Method | SymKind::Term
                    )
                })
                .collect();
            if !terms.is_empty() {
                found = terms;
            }
        }
        // The conversion a view inserted, when one was: the member it produced
        // may be declared at the type parameters of the *value* the conversion
        // was imported from, which only that prefix can fill in.
        let mut ext_conv = SymbolId::NONE;
        if found.is_empty() {
            if let Some((conv, member, to)) = self.search_extension(&recv_ty, &name, tree.span) {
                ext_conv = conv;
                let span = qual.span;
                let old = std::mem::replace(qual.as_mut(), Tree::dummy(TreeKind::Empty));
                let from = old.ty.clone();
                let fun = self.ref_implicit(conv, span);
                let applied = Tree {
                    id: old.id,
                    span,
                    kind: TreeKind::Apply {
                        fun: Box::new(fun),
                        args: vec![old],
                    },
                    ty: to.clone(),
                    sym: conv,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                **qual = self.fill_conv_implicits(conv, &from, applied, span);
                found = if let Some(cls) = self.st.class_sym_of(&to) {
                    self.st.lookup_member(cls, &name)
                } else {
                    vec![member]
                };
                // The member now belongs to the conversion's result, so
                // substitution must see `to`, not the original receiver.
                recv_ty = to.clone();
            }
        }
        if found.is_empty() && self.is_dynamic_receiver(&qual.ty) {
            if matches!(pt, Type::Method { .. }) {
                // `d.foo(args)`: type_apply rewrites to applyDynamic.
                tree.ty = Type::Error;
                return;
            }
            self.rewrite_select_dynamic(tree, pt);
            return;
        }
        // No second `supply_from_pickle` here: the search above already ran it
        // for this receiver, and `PickleSupply::complete_named` memoizes
        // `(class, name)` — asking twice returns the class's *raw* members
        // under that name, past the `STATIC` filter that
        // "static Java members are not inherited" depends on
        // (`java.lang.Integer.valueOf(3).parseInt("12")` stopped being an
        // error). The one path that moves the receiver, `search_extension`,
        // leaves `found` non-empty anyway.
        if found.is_empty() {
            // `Any`'s members belong to every type, including the ones with no
            // class symbol to walk: `(f: Int => String).asInstanceOf[…]`.
            found = self.st.lookup_member(self.st.any_sym, &name);
        }
        if found.is_empty() {
            // nsc reports the cause once: a selection on a receiver that is
            // already an error adds nothing.
            if !qual.ty.is_error()
                && !self.report_internal_universe_macro(tree.span, &name, {
                    let owner = self.st.class_sym_of(&qual.ty).unwrap_or(SymbolId::NONE);
                    self.is_reflect_universe(owner)
                })
            {
                self.error(
                    tree.span,
                    format!(
                        "value {name} is not a member of {}",
                        self.st.display_type(&qual.ty)
                    ),
                );
            }
            tree.ty = Type::Error;
            return;
        }
        // nsc's `nonLocalMember`: a `private[this]` member is not a member of
        // any prefix but `this`. `class JdbcFunction(name: String) extends
        // FunctionSymbol(name)` keeps its parameter as one, and `o.name` on
        // another instance means the inherited `val` -- letting the parameter
        // shadow it made every such selection inaccessible.
        if !self.prefix_is_this(Some(qual.as_ref())) && found.len() > 1 {
            let non_local: Vec<SymbolId> = found
                .iter()
                .copied()
                .filter(|s| {
                    let f = self.st.get(*s).flags;
                    !(f.contains(Flags::PRIVATE) && f.contains(Flags::LOCAL))
                })
                .collect();
            if !non_local.is_empty() {
                found = non_local;
            }
        }
        found = self.drop_overridden(found);
        // `x.toString` finds both `Any.toString` and `Int.toString`; they have
        // the same type, so this is one member, not an ambiguous overload.
        if found.len() > 1 {
            let first_ty = self.st.get(found[0]).ty.clone();
            if found
                .iter()
                .all(|&s| self.st.get(s).ty == first_ty && !first_ty.is_no_type())
            {
                found.truncate(1);
            }
        }
        found.retain(|s| self.accessible(*s, Some(qual.as_ref())));
        self.note_companion_access(&found);
        if found.is_empty() {
            self.error(
                tree.span,
                format!(
                    "value {name} cannot be accessed as a member of {} from {}",
                    self.st.display_type(&qual.ty),
                    self.access_from_name()
                ),
            );
            tree.ty = Type::Error;
            return;
        }
        // Term position prefers the companion module (and methods/vals) over
        // the class of the same name, matching `type_ident`.
        let terms: Vec<SymbolId> = found
            .iter()
            .copied()
            .filter(|s| {
                matches!(
                    self.st.get(*s).kind,
                    SymKind::Module | SymKind::Method | SymKind::Term
                )
            })
            .collect();
        if !terms.is_empty() {
            found = terms;
        }
        for s in found.iter().copied() {
            self.complete_lazy_sig(s, tree.span);
        }
        if found.len() > 1 {
            self.record_overload_group(&found, &name);
        }
        // Only the receivers `subst_as_seen_from` cannot walk. A *class*
        // receiver it walks properly, parent by parent; reading the member a
        // second time at the receiver's own arguments assumes the declaring
        // class's parameters sit at the same positions, which slick's
        // `BaseJoinQuery[E1, E2, U1, U2, C, B1, B2] <: Query[+E, U, C[_]]`
        // disproves -- `Query.map`'s `Query[G, T, C]` came back as
        // `Query[G, T, U1]`. And where the receiver names the enclosing
        // class's own parameters (`stdJoin` inside `Query` builds a
        // `BaseJoinQuery[E, E2, …]`), the second pass is not even idempotent:
        // `f: E => F` became `f: ((E, E2), E2) => F`.
        // A type annotation says nothing about the members: `x._1` on a
        // `(String, Int) @uncheckedVariance` -- what slick's
        // `ConstArray.toSet: HashSet[T @uncheckedVariance]` hands its callers
        // -- is the same `_1` as on the bare tuple. Without peeling it here,
        // `subst_args` stayed empty (`subst_as_seen_from` has no tuple case;
        // that is what this list is for) and `Tuple2._1` kept its declared
        // `T1`, so `referenced.map(_._1)` came out `HashSet[T1]`.
        let subst_args: Vec<Type> = match peel_type_annot(&recv_ty) {
            Type::Tuple(ts) => ts.clone(),
            // `FunctionN`'s parameters are `T1 … Tn, R`, in that order.
            Type::Function { params, ret } => {
                let mut a = params.clone();
                a.push((**ret).clone());
                a
            }
            _ => Vec::new(),
        };
        let subst = |ty: Type| -> Type {
            // `import seq.integral._; increment < zero` is
            // `integral.mkOrderingOps(increment) < zero`, and `OrderingOps#<`
            // takes an `Ordering`'s `T`, not one of its own: the receiver here
            // is `OrderingOps`, which has no argument to read it off.
            let ty = if ext_conv.is_none() {
                ty
            } else {
                self.at_import_prefix_of(ext_conv, &ty).unwrap_or(ty)
            };
            let ty = self.st.subst_as_seen_from(&recv_ty, &ty);
            if !subst_args.is_empty() {
                if let Some(owner) = found.first().map(|s| self.st.get(*s).owner) {
                    return self.st.subst_tparams(owner, &subst_args, &ty);
                }
            }
            ty
        };
        let expand = |ty: Type| -> Type {
            // For `super.m` the enclosing class comes first: slick's
            // `SQLiteProfile` mixes in `MultipleRowsPerStatementSupport`, whose
            // `override type RowsPerStatement = slick.jdbc.RowsPerStatement`
            // is what the inherited `insertAll(values, rowsPerStatement:
            // RowsPerStatement)` takes. Reading the parameter off the parent
            // alone leaves the *abstract* member `>: One.type <: RowsPerStatement`,
            // which no argument of the concrete type conforms to.
            let ty = match super_this {
                Some(c) => self.st.expand_type_members(c, &ty),
                None => ty,
            };
            self.st.expand_in_type(&recv_ty, &ty)
        };
        if found.len() == 1 {
            let s = found[0];
            tree.sym = s;
            let ty = expand(subst(self.st.get(s).ty.clone()));
            let ty = self.maybe_auto_apply(ty, pt);
            tree.ty = self.instantiate_parameterless(s, ty, pt);
            if let Type::Array(elem) = &qual.ty {
                if name == "apply" {
                    tree.ty = Type::Method {
                        paramss: vec![vec![Type::Int]],
                        ret: Box::new((**elem).clone()),
                    };
                } else if name == "update" {
                    tree.ty = Type::Method {
                        paramss: vec![vec![Type::Int, (**elem).clone()]],
                        ret: Box::new(Type::Unit),
                    };
                } else if name == "clone" && self.st.get(s).owner == self.st.array_sym {
                    // `def clone(): Array[T]` — the element type is the
                    // receiver's, exactly as for `apply`.
                    tree.ty = Type::Method {
                        paramss: vec![vec![]],
                        ret: Box::new(Type::Array(Box::new((**elem).clone()))),
                    };
                }
            }
        } else {
            tree.sym = found[0];
            // As-seen-from, like the single-member branch above: an
            // alternative inherited from a generic parent is only itself once
            // the receiver's arguments are in
            // (`PartialFunction[Int, A].apply` is `apply(Int): A`, not
            // `apply(A): B`).
            let alts: Vec<(SymbolId, Type)> = found
                .iter()
                .map(|s| (*s, expand(subst(self.st.get(*s).ty.clone()))))
                .collect();
            self.overload_member_types.insert(found[0].0, alts.clone());
            let ov = Type::Overload(alts.clone().into_iter().map(|(_, t)| t).collect());
            tree.ty = self.maybe_auto_apply(ov, pt);
            if !matches!(tree.ty, Type::Overload(_)) {
                if let Some(id) = found
                    .iter()
                    .copied()
                    .find(|&s| self.is_nullary_method_sym(s))
                    .or_else(|| {
                        found
                            .iter()
                            .copied()
                            .find(|&s| self.is_parameterless_sym(s))
                    })
                {
                    tree.sym = id;
                    // Value position dropped the alternatives that take
                    // parameters (SLS 6.26.3), but explicit type arguments are
                    // read *before* that rule applies, so a `TypeApply` above
                    // has to be able to get back to the set. Record it under
                    // the surviving symbol too -- the key the caller has.
                    if id != found[0] {
                        self.overload_member_types.insert(id.0, alts);
                        self.record_overload_group(&found, &name);
                        if let Some(g) = self.overload_groups.get(&found[0].0).cloned() {
                            self.overload_groups.insert(id.0, g);
                        }
                    }
                }
            }
        }
        // A function value's `apply` is the function itself. The prelude's
        // `FunctionN.apply` is declared over erased parameters, so selecting it
        // through a `Type::Function` receiver produced `(Any)Any` and
        // `f.apply(xs)` came out as `Any` where `f(xs)` was exact.
        if name == "apply" {
            if let Type::Function { params, ret } = &recv_ty {
                tree.ty = Type::Method {
                    paramss: vec![params.clone()],
                    ret: ret.clone(),
                };
            }
        }
    }

    /// Remember `found` when re-deriving it from `found[0]`'s owner would lose
    /// alternatives, so `resolve_overload` can use the real set.
    ///
    /// The common case -- every alternative reachable from the head's owner --
    /// records nothing, and resolution keeps re-deriving as before.
    pub(crate) fn record_overload_group(&mut self, found: &[SymbolId], name: &str) {
        let Some(&head) = found.first() else { return };
        if found.len() < 2 {
            return;
        }
        let owner = self.st.get(head).owner;
        let derived = self.st.lookup_member(owner, name);
        if found.iter().all(|s| derived.contains(s)) {
            return;
        }
        self.overload_groups.insert(head.0, found.to_vec());
    }

    /// The alternatives `fun_sym` stands for: the recorded set when the
    /// overload spans owners, otherwise everything the owner declares.
    pub(crate) fn overload_alternatives(&self, fun_sym: SymbolId, name: &str) -> Vec<SymbolId> {
        if let Some(g) = self.overload_groups.get(&fun_sym.0) {
            if self.st.get(fun_sym).name == name {
                return g.clone();
            }
        }
        let owner = self.st.get(fun_sym).owner;
        self.st.lookup_member(owner, name)
    }

    /// Prefer a definition on a subclass over the inherited member it overrides.
    pub(crate) fn drop_overridden(&self, found: Vec<SymbolId>) -> Vec<SymbolId> {
        if found.len() <= 1 {
            return found;
        }
        let found = self.collapse_pickled_copies(found);
        let kept: Vec<SymbolId> = found
            .iter()
            .copied()
            .filter(|&s| {
                let owner = self.st.get(s).owner;
                !found.iter().any(|&other| {
                    if other == s {
                        return false;
                    }
                    let oo = self.st.get(other).owner;
                    if oo == owner {
                        return false;
                    }
                    // A *declaration* is never an alternative next to a
                    // definition of the same signature, however the two
                    // reached this class. The owner test below asks for
                    // `other`'s class to be below `s`'s, which is exactly what
                    // a self type does not give: gitbucket writes
                    //
                    // ```scala
                    // trait Profile { val profile: BlockingJdbcProfile }
                    // trait ProfileProvider { self: Profile =>
                    //   lazy val profile = DatabaseConfig.slickDriver }
                    // trait CoreProfile extends ProfileProvider with Profile
                    // ```
                    //
                    // and `Profile.profile.blockingApi` was then selected on
                    // `<overload BlockingJdbcProfile | ...>`, taking every
                    // slick `Session` in the program with it. In the class
                    // that has both, one implements the other; nsc's
                    // linearization sees a single member.
                    if self.is_deferred_member(s)
                        && !self.is_deferred_member(other)
                        && self.same_signature(other, s)
                    {
                        return true;
                    }
                    let child = Type::Class {
                        sym: oo,
                        args: vec![],
                    };
                    let parent = Type::Class {
                        sym: owner,
                        args: vec![],
                    };
                    self.st.is_sub_type(&child, &parent)
                        // Inheriting is not overriding: nsc keeps `f(Int)`
                        // declared on the parent as an alternative of `f`
                        // alongside a `f(String)` the subclass adds. Only a
                        // *matching* signature replaces the inherited one.
                        && self.same_signature(other, s)
                })
            })
            .collect();
        // Dropping *every* candidate is never what this rule means -- it
        // removes shadowed duplicates, and a set that eliminates itself
        // mutually (seen first with agent/tail2's supplied jar implicits next
        // to prelude twins) must fall back to what it was given rather than
        // leave the caller to index into nothing.
        if kept.is_empty() {
            return found;
        }
        kept
    }

    /// One pickled declaration reached through two classes is one member.
    ///
    /// `PickleSupply` installs an inherited member on the class the lookup
    /// asked about, because that is where the typer has to find it. Which
    /// class asks first is a property of the *program*, so `IterableOps.map`
    /// gets copied onto `immutable.Seq` for `xs.map` on a `Seq` and onto
    /// `collection.IndexedSeq` for `ys.map` on an `IndexedSeq` — and
    /// `immutable.IndexedSeq`, which has both above it and neither below the
    /// other, then offers two `map`s that differ only in the vocabulary each
    /// copy was rewritten into (`Seq[B]` vs `IndexedSeq[B]`). `drop_overridden`
    /// cannot relate them, specificity cannot separate them, and every
    /// `xs.map(f)` on such a receiver was `ambiguous overload`.
    ///
    /// nsc sees one `IterableOps.map`. So does this: copies of one pickled
    /// declaration collapse to the first, which is the one `lookup_member`
    /// reached first and so the nearest to the receiver.
    fn collapse_pickled_copies(&self, found: Vec<SymbolId>) -> Vec<SymbolId> {
        if !found
            .iter()
            .any(|&s| !self.st.get(s).pickled_origin.is_empty())
        {
            return found;
        }
        let mut seen: HashSet<&str> = HashSet::new();
        found
            .iter()
            .copied()
            .filter(|&s| {
                let origin = self.st.get(s).pickled_origin.as_str();
                origin.is_empty() || seen.insert(origin)
            })
            .collect()
    }

    /// nsc `matchingSymbols`: does `sub` override `base`? Both are members of
    /// the same name, so what decides it is the parameter list. A
    /// parameterless `val` matches a nullary `def` (that is how `override val
    /// sqlType` implements `def sqlType: Int`).
    fn same_signature(&self, sub: SymbolId, base: SymbolId) -> bool {
        let sub_ps = flat_param_types(&self.st.get(sub).ty);
        let base_ps = flat_param_types(&self.st.get(base).ty);
        if sub_ps.len() != base_ps.len() {
            return false;
        }
        // The parent declares its members in its own type parameters; a member
        // seen from the subclass has them substituted away. Rather than
        // reconstructing the prefix here, a parameter mentioning a type
        // parameter matches anything -- an overload that differs only inside a
        // type parameter is not one nsc can distinguish either.
        sub_ps.iter().zip(&base_ps).all(|(a, b)| {
            a == b
                || a.is_no_type()
                || b.is_no_type()
                || sig_has_abstract_type(a)
                || sig_has_abstract_type(b)
        })
    }

    fn access_from_name(&self) -> String {
        if self.st.this_class.is_none() {
            "<none>".into()
        } else {
            self.st.get(self.st.this_class).name.clone()
        }
    }

    /// The synthetic `copy` of a case class, if it still has one (a written
    /// `def copy` replaces it).
    fn synthetic_copy(&self, class_id: SymbolId) -> Option<SymbolId> {
        self.st
            .lookup_member(class_id, "copy")
            .into_iter()
            .find(|&s| self.st.get(s).flags.contains(Flags::SYNTHETIC))
    }

    /// `p.copy(x = 1)` is rewritten straight into a constructor call, so the
    /// access check `type_select` does for an ordinary member never runs on
    /// `copy` itself. Under `-Xsource-features:case-apply-copy-access` that
    /// matters: scalac rejects `v.copy(x = 2)` for both
    /// `case class C private (x: Int)` and `case class D protected (x: Int)`.
    /// Reports and returns true when the call is not allowed.
    fn case_copy_access_error(
        &mut self,
        class_id: SymbolId,
        prefix: Option<&Tree>,
        span: Span,
    ) -> bool {
        let Some(copy) = self.synthetic_copy(class_id) else {
            return false;
        };
        if self.accessible(copy, prefix) {
            // Legal, but possibly from a *different class file* — a nested or
            // anonymous class inside the case class is inside its scope and
            // outside its classfile, and `ACC_PRIVATE` there is an
            // `IllegalAccessError`. `expand_private_names` cannot see this
            // one: the synthetic `copy` has no `DefDef` to walk.
            if self.st.this_class != class_id {
                self.st.get_mut(copy).access_widened = true;
            }
            return false;
        }
        let owner = self.st.get(class_id).name.clone();
        let from = self.access_from_name();
        self.error(
            span,
            format!("value copy cannot be accessed as a member of {owner} from {from}"),
        );
        true
    }

    /// nsc-style accessibility. `private[this]` requires a `this` prefix.
    /// `protected[C]` is protected plus everything nested in `C`.
    fn accessible(&self, sym: SymbolId, prefix: Option<&Tree>) -> bool {
        if sym.is_none() {
            return true;
        }
        let s = self.st.get(sym);
        let flags = s.flags;
        let restricted = flags.contains(Flags::PRIVATE)
            || flags.contains(Flags::PROTECTED)
            || s.private_within.is_some();
        if !restricted {
            return true;
        }
        let owner = s.owner;
        let current = self.st.this_class;
        if flags.contains(Flags::PRIVATE) && flags.contains(Flags::LOCAL) {
            return self.prefix_is_this(prefix) && self.nested_in(current, owner);
        }
        if flags.contains(Flags::PRIVATE) {
            if let Some(w) = &s.private_within {
                return self.access_within_of(current, w, owner);
            }
            return self.nested_in(current, owner);
        }
        if flags.contains(Flags::PROTECTED) {
            // Java `protected` is also package-private (JLS / nsc Java interop).
            if flags.contains(Flags::JAVA) && self.java_same_package(current, owner) {
                return true;
            }
            let by_qual = s
                .private_within
                .as_ref()
                .map(|w| self.access_within_of(current, w, owner))
                .unwrap_or(false);
            let by_sub = self.protected_subclass_ok(current, owner, prefix);
            // nsc's `accessWithin(ab) || accessWithinLinked(ab)` with
            // `ab = sym.owner`: being lexically inside the owner *or inside
            // its companion* grants access whatever the qualifier says.
            // slick's `object ResultConverterCompiler { protected lazy val
            // logger }` is read from the companion `trait
            // ResultConverterCompiler`, which no subclass rule covers.
            let by_companion = self.nested_in(current, owner);
            return by_qual || by_sub || by_companion;
        }
        if let Some(w) = &s.private_within {
            return self.access_within_of(current, w, owner);
        }
        true
    }

    fn prefix_is_this(&self, prefix: Option<&Tree>) -> bool {
        match prefix {
            None => true,
            Some(t) => matches!(t.kind, TreeKind::This { .. } | TreeKind::Super { .. }),
        }
    }

    fn nested_in(&self, current: SymbolId, owner: SymbolId) -> bool {
        if owner.is_none() {
            return true;
        }
        let mut c = current;
        while !c.is_none() {
            // A class and its companion object share private access, so at
            // every enclosing level the companion counts as the same scope.
            if c == owner || self.companion_scope(c) == Some(owner) {
                return true;
            }
            c = self.st.get(c).owner;
        }
        false
    }

    /// `nested_in` without the companion rule: true enclosure only.
    fn enclosed_by(&self, current: SymbolId, owner: SymbolId) -> bool {
        if owner.is_none() {
            return true;
        }
        let mut c = current;
        while !c.is_none() {
            if c == owner {
                return true;
            }
            c = self.st.get(c).owner;
        }
        false
    }

    /// Mark a `private` member read across the companion boundary; the JVM
    /// would reject `ACC_PRIVATE` there, so the backend widens it.
    fn note_companion_access(&mut self, members: &[SymbolId]) {
        for &m in members {
            let s = self.st.get(m);
            if !s.flags.contains(Flags::PRIVATE) || s.flags.contains(Flags::LOCAL) {
                continue;
            }
            let owner = s.owner;
            if !self.enclosed_by(self.st.this_class, owner) {
                self.st.get_mut(m).access_widened = true;
            }
        }
    }

    /// The companion of a class (its module class) or of a module class (the
    /// class of the same name), for access checks.
    fn companion_scope(&self, c: SymbolId) -> Option<SymbolId> {
        let s = self.st.get(c);
        match s.kind {
            SymKind::Class => {
                let m = self.st.companion_module(c)?;
                Some(self.st.module_class_of(m))
            }
            SymKind::ModuleClass => {
                let name = s.name.trim_end_matches('$').to_string();
                let owner = s.owner;
                self.st
                    .get(owner)
                    .members
                    .iter()
                    .copied()
                    .find(|&m| self.st.get(m).kind == SymKind::Class && self.st.get(m).name == name)
            }
            _ => None,
        }
    }

    /// `private[X]` names an enclosing class or package **of the definition**,
    /// so `from` -- the member's owner -- is where the name is resolved. A
    /// plain lookup found `scala.util` for slick's `private[util]` and every
    /// use of `ConstArray.copySliceTo` was inaccessible.
    fn access_within_of(&self, current: SymbolId, name: &str, from: SymbolId) -> bool {
        let mut boundary = SymbolId::NONE;
        let mut c = from;
        while !c.is_none() {
            if self.st.get(c).name == name || self.st.get(c).name.trim_end_matches('$') == name {
                boundary = c;
                break;
            }
            c = self.st.get(c).owner;
        }
        let boundary = if boundary.is_none() {
            self.resolve_access_boundary(name)
        } else {
            boundary
        };
        if boundary.is_none() {
            return false;
        }
        self.nested_in(current, boundary)
            || self.st.get(current).name == name
            || self.st.get(current).name.trim_end_matches('$') == name
    }

    fn resolve_access_boundary(&self, name: &str) -> SymbolId {
        for id in self.st.lookup(name) {
            if matches!(
                self.st.get(id).kind,
                SymKind::Class | SymKind::ModuleClass | SymKind::Module | SymKind::Package
            ) {
                return match self.st.get(id).kind {
                    SymKind::Module => self.st.module_class_of(id),
                    _ => id,
                };
            }
        }
        let mut c = self.st.this_class;
        while !c.is_none() {
            if self.st.get(c).name == name || self.st.get(c).name.trim_end_matches('$') == name {
                return c;
            }
            c = self.st.get(c).owner;
        }
        SymbolId::NONE
    }

    fn protected_subclass_ok(
        &self,
        current: SymbolId,
        owner: SymbolId,
        prefix: Option<&Tree>,
    ) -> bool {
        if current.is_none() || owner.is_none() {
            return false;
        }
        // nsc weighs the rule against every *enclosing* class, not only the
        // innermost one: `new DDL { … self.createPhase1 … }` written inside
        // `DDL` itself is in `DDL`'s template, so the prefix only has to be a
        // `DDL`, not an instance of the anonymous class.
        let mut c = current;
        while !c.is_none() {
            if self.st.get(c).is_class_like() && self.protected_ok_in(c, owner, prefix) {
                return true;
            }
            c = self.st.get(c).owner;
        }
        false
    }

    fn protected_ok_in(&self, current: SymbolId, owner: SymbolId, prefix: Option<&Tree>) -> bool {
        let cur_ty = self.st.type_of_class(current);
        let own_ty = self.st.type_of_class(owner);
        if current != owner && !self.st.is_sub_type(&cur_ty, &own_ty) {
            return false;
        }
        match prefix {
            None => true,
            Some(t) if matches!(t.kind, TreeKind::This { .. } | TreeKind::Super { .. }) => true,
            Some(t) => self.st.is_sub_type(&t.ty, &cur_ty),
        }
    }

    fn java_same_package(&self, current: SymbolId, member_owner: SymbolId) -> bool {
        let a = self.enclosing_package(current);
        let b = self.enclosing_package(member_owner);
        !a.is_none() && a == b
    }

    pub(crate) fn enclosing_package(&self, mut id: SymbolId) -> SymbolId {
        while !id.is_none() {
            if self.st.get(id).kind == SymKind::Package {
                return id;
            }
            id = self.st.get(id).owner;
        }
        self.st.root
    }

    /// A select on a *class name* in term position that no argument list
    /// matched, widened with the companion object's members.
    ///
    /// nsc reads a class name in term position as its companion object, so
    /// only the object's members are in scope there. This typer keeps the
    /// class symbol as the receiver and lets `supply_from_pickle` install
    /// companion members on the module class, which works as long as nothing
    /// by that name sits on the class itself. `scala.math.BigDecimal` breaks
    /// that: it declares an *instance* `apply(MathContext)`, and once
    /// completion has installed it -- which it can only do after something in
    /// the run has pulled `java.math.MathContext` in, since the parameter is
    /// unmappable before that -- every later `BigDecimal(...)` finds that one
    /// method, stops there, and never sees the companion's seven `apply`
    /// overloads. The same program then compiled or not depending on the
    /// order of unrelated statements.
    ///
    /// Nothing is loaded or installed here: both scopes are already in the
    /// table, and this only runs on a path that is otherwise about to report
    /// an error, so it can turn a rejection into a resolution and nothing else.
    pub(crate) fn widen_with_companion(&mut self, fun: &mut Tree) -> bool {
        let TreeKind::Select { qual, name } = &fun.kind else {
            return false;
        };
        // A value receiver keeps the class's own scope; only a bare class name
        // stands for its companion.
        if qual.sym.is_none() || self.st.get(qual.sym).kind != SymKind::Class {
            return false;
        }
        let name = name.clone();
        let recv_ty = qual.ty.clone();
        let Some(cls) = self.st.class_sym_of(&recv_ty) else {
            return false;
        };
        let module = self.st.companion_module(cls);
        let Some(module) = module else {
            return false;
        };
        let mcls = self.st.module_class_of(module);
        let before = match &fun.ty {
            Type::Overload(alts) => alts.len(),
            Type::Method { .. } => 1,
            _ => return false,
        };
        let mut found = self.st.lookup_member(cls, &name);
        for s in self.st.lookup_member(mcls, &name) {
            if !found.contains(&s) {
                found.push(s);
            }
        }
        found.retain(|&s| self.st.get(s).kind == SymKind::Method);
        found = self.drop_overridden(found);
        if found.len() <= before || found.len() < 2 {
            return false;
        }
        self.record_overload_group(&found, &name);
        let subst_args: Vec<Type> = match &recv_ty {
            Type::Class { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let owner = self.st.get(found[0]).owner;
        let alts: Vec<(SymbolId, Type)> = found
            .iter()
            .map(|&s| {
                let t = self.st.subst_as_seen_from(&recv_ty, &self.st.get(s).ty);
                let t = if subst_args.is_empty() {
                    t
                } else {
                    self.st.subst_tparams(owner, &subst_args, &t)
                };
                (s, self.st.expand_in_type(&recv_ty, &t))
            })
            .collect();
        self.overload_member_types.insert(found[0].0, alts.clone());
        fun.ty = Type::Overload(alts.into_iter().map(|(_, t)| t).collect());
        fun.sym = found[0];
        true
    }

    /// The pickle's alternatives for a *module* receiver whose overload set is
    /// only what the prelude wrote by hand.
    ///
    /// `BigDecimal(3L)` used to reach the companion the long way round: the
    /// term `BigDecimal` was the trait/class symbol, `apply` was not a member
    /// of it, and the `found.is_empty()` branch in `type_select` read the
    /// companion's seven alternatives out of the pickle on the way past.
    /// Resolving the alias to the module directly (`prelude_ordsummon`, which
    /// is what `val BigDecimal = scala.math.BigDecimal` means) short-circuits
    /// that: the module class already carries the three `apply`s the prelude
    /// writes, `found` is not empty, and nothing was ever read -- so
    /// `BigDecimal(3L)` and `BigDecimal(BigInt(6))` became "no matching
    /// overload".
    ///
    /// Asked only here, on the path that is otherwise about to report that
    /// error, and only *adding*: `PickleSupply` declines a copy of a
    /// hand-written prelude member (`agent/setapply`), so the alternatives that
    /// arrive are the ones with an erasure the prelude does not already have.
    pub(crate) fn widen_module_from_pickle(&mut self, fun: &mut Tree) -> bool {
        if !self.library_abi {
            return false;
        }
        let TreeKind::Select { qual, name } = &fun.kind else {
            return false;
        };
        let name = name.clone();
        let recv_ty = qual.ty.clone();
        let Some(mcls) = self.st.class_sym_of(&recv_ty) else {
            return false;
        };
        if self.st.get(mcls).kind != SymKind::ModuleClass {
            return false;
        }
        let before = self.st.lookup_member(mcls, &name).len();
        if self.supply_from_pickle_class(mcls, &name).is_empty() {
            return false;
        }
        let mut found = self.st.lookup_member(mcls, &name);
        found.retain(|&s| self.st.get(s).kind == SymKind::Method);
        found = self.drop_overridden(found);
        if found.len() <= before || found.is_empty() {
            return false;
        }
        self.record_overload_group(&found, &name);
        let alts: Vec<(SymbolId, Type)> = found
            .iter()
            .map(|&s| {
                let t = self.st.subst_as_seen_from(&recv_ty, &self.st.get(s).ty);
                (s, self.st.expand_in_type(&recv_ty, &t))
            })
            .collect();
        self.overload_member_types.insert(found[0].0, alts.clone());
        fun.ty = Type::Overload(alts.into_iter().map(|(_, t)| t).collect());
        fun.sym = found[0];
        true
    }

    /// [`Self::widen_module_from_pickle`] for the `apply` *sugar*:
    /// `cats.effect.IO(fa)` means `IO.apply(fa)`, but the tree's function is
    /// the reference to the object, not a `Select` of `apply`, so that pass
    /// declines it — its `Select` is `cats.effect.IO`, whose qualifier is a
    /// package.
    ///
    /// It matters wherever the class file reader got to a Scala companion
    /// first — [`Self::load_companion_module`] warming a jar class's implicit
    /// scope, or the class file of the class itself, which carries a *static
    /// forwarder* for every companion method. A class file cannot write a
    /// by-name parameter, so `IO.apply(thunk: => A)` reads back as
    /// `apply(Function0[A]): IO[A]` either way, and the on-demand pickle path
    /// never corrects it: that runs only when a lookup finds *nothing*, and
    /// the erased member is something. `cats.effect.IO(fa)` with
    /// `fa: Future[R]` was then "no matching overload" — but only once
    /// something earlier in the run had read one of those class files, which
    /// is what made it look like a failure only the whole program could
    /// produce (slick's `dbio/DBIOAction.scala`).
    ///
    /// Asked only here, on the path that is otherwise about to report that
    /// error, so a companion is never completed speculatively — the reason
    /// `load_companion_module` stops at the implicits in the first place.
    pub(crate) fn retry_module_apply_from_pickle(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        if !self.library_abi {
            return false;
        }
        let TreeKind::Apply { fun, .. } = &tree.kind else {
            return false;
        };
        // The module class this call is really about. Either the reference
        // still stands as the function, or the sugar has already collapsed to
        // the single `apply` the receiver had — which is exactly the erased
        // alternative that does not fit.
        let mcls = match &fun.ty {
            Type::ModuleRef(c) => *c,
            Type::Class { sym, .. } => *sym,
            _ => {
                if fun.sym.is_none() || self.st.get(fun.sym).name != "apply" {
                    return false;
                }
                let owner = self.st.get(fun.sym).owner;
                match self.st.get(owner).kind {
                    SymKind::ModuleClass => owner,
                    // scalac also emits every companion method as a *static
                    // forwarder* on the class, and the class file reader
                    // installs those on the class exactly as it installs the
                    // companion's own — erased either way. `cats.effect.IO(fa)`
                    // picked the forwarder on `cats/effect/IO`; the pickle for
                    // it is the companion's.
                    SymKind::Class => {
                        self.load_companion_module(owner);
                        let jvm = format!("{}$", self.st.get(owner).jvm_name);
                        match self
                            .st
                            .companion_module(owner)
                            .map(|m| self.st.module_class_of(m))
                            .or_else(|| crate::classpath::find_by_jvm(&self.st, &jvm))
                        {
                            Some(c) => c,
                            None => return false,
                        }
                    }
                    _ => return false,
                }
            }
        };
        if mcls.is_none() || self.st.get(mcls).kind != SymKind::ModuleClass {
            return false;
        }
        let before = self.st.lookup_member(mcls, "apply").len();
        if self.supply_from_pickle_class(mcls, "apply").is_empty()
            || self.st.lookup_member(mcls, "apply").len() <= before
        {
            // Nothing new: the pickle has already been read for this name, so
            // the retry below would type exactly the same tree again.
            return false;
        }
        let TreeKind::Apply { fun, .. } = &mut tree.kind else {
            return false;
        };
        fun.ty = Type::NoType;
        fun.sym = SymbolId::NONE;
        tree.ty = Type::NoType;
        tree.sym = SymbolId::NONE;
        self.type_expr(tree, pt);
        true
    }

    /// When a member exists on the receiver (e.g. `Int.+`) but the argument
    /// types do not match, try an implicit conversion that *does* have the
    /// method (`any2stringadd` for `1 + "x"`).
    pub(crate) fn rewrite_apply_extension(&mut self, fun: &mut Tree) -> bool {
        let TreeKind::Select { qual, name } = &mut fun.kind else {
            return false;
        };
        let Some((conv, member, to)) = self.search_extension(&qual.ty, name, fun.span) else {
            return false;
        };
        let span = qual.span;
        let old = std::mem::replace(qual.as_mut(), Tree::dummy(TreeKind::Empty));
        let from = old.ty.clone();
        let conv_fun = self.ref_implicit(conv, span);
        let applied = Tree {
            id: old.id,
            span,
            kind: TreeKind::Apply {
                fun: Box::new(conv_fun),
                args: vec![old],
            },
            ty: to,
            sym: conv,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        **qual = self.fill_conv_implicits(conv, &from, applied, span);
        fun.sym = member;
        let mty = self.st.get(member).ty.clone();
        fun.ty = self.at_import_prefix_of(conv, &mty).unwrap_or(mty);
        true
    }

    fn is_dynamic_receiver(&self, ty: &Type) -> bool {
        if let Type::Named { name, .. } = ty {
            if name == "Dynamic" || name.ends_with(".Dynamic") {
                return true;
            }
        }
        let mut work = Vec::new();
        if let Some(c) = self.st.class_sym_of(ty) {
            work.push(c);
        }
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            let s = self.st.get(id);
            if s.name == "Dynamic"
                || s.jvm_name == "scala/Dynamic"
                || s.jvm_name.ends_with("/Dynamic")
            {
                return true;
            }
            for p in s.parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                } else if let Type::Named { name, .. } = &p {
                    if name == "Dynamic" || name.ends_with(".Dynamic") {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn receiver_has_term(&self, ty: &Type, name: &str) -> bool {
        match ty {
            Type::Refined { decls, parents } => {
                let in_decl = decls.iter().any(|d| {
                    matches!(
                        d,
                        scala_rs_parser::RefineDecl::Def { name: n, .. }
                            | scala_rs_parser::RefineDecl::Val { name: n, .. }
                            if n == name
                    )
                });
                if in_decl {
                    return true;
                }
                parents.iter().any(|p| self.receiver_has_term(p, name))
            }
            Type::ModuleRef(id) => !self.st.lookup_member(*id, name).is_empty(),
            _ => {
                if let Some(o) = self.st.class_sym_of(ty) {
                    if !self.st.lookup_member(o, name).is_empty() {
                        return true;
                    }
                }
                name == "toString"
            }
        }
    }

    fn dynamics_feature_error(&mut self, span: Span, method: &str) {
        self.error(
            span,
            format!(
                "Dynamic method {method} needs to be enabled by making the implicit value scala.language.dynamics visible"
            ),
        );
    }

    pub(crate) fn check_implicit_conversions_feature(&mut self, span: Span, name: &str) {
        if self.language_implicit_conversions {
            return;
        }
        self.warning(
            span,
            format!(
                "implicit conversion method {name} should be enabled by making the implicit value scala.language.implicitConversions visible"
            ),
        );
    }

    fn rewrite_select_dynamic(&mut self, tree: &mut Tree, pt: &Type) {
        if !self.language_dynamics {
            self.dynamics_feature_error(tree.span, "selectDynamic");
            tree.ty = Type::Error;
            return;
        }
        let span = tree.span;
        let id = tree.id;
        let (qual, dyn_name) = match &mut tree.kind {
            TreeKind::Select { qual, name } => (
                std::mem::replace(qual, Box::new(Tree::dummy(TreeKind::Empty))),
                name.clone(),
            ),
            _ => return,
        };
        let name_lit = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Literal {
                lit: Lit::String(dyn_name),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let sel = Tree {
            id,
            span,
            kind: TreeKind::Select {
                qual,
                name: "selectDynamic".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        tree.kind = TreeKind::Apply {
            fun: Box::new(sel),
            args: vec![name_lit],
        };
        self.type_apply(tree, pt);
    }

    /// nsc `convertToAssignment`: `x += 1` becomes `x = x.+(1)` when `+=` is
    /// not a member and the receiver is assignable. A real `def +=` wins.
    pub(crate) fn try_rewrite_assignment_op(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let name = match &tree.kind {
            TreeKind::Apply { fun, .. } => match &fun.kind {
                TreeKind::Select { name, .. } if is_assignment_op(name) => name.clone(),
                _ => return false,
            },
            _ => return false,
        };
        let span = tree.span;
        let id = tree.id;
        let TreeKind::Apply { fun, args } = &mut tree.kind else {
            return false;
        };
        let fun_id = fun.id;
        let fun_span = fun.span;
        let TreeKind::Select { qual, .. } = &mut fun.kind else {
            return false;
        };
        self.type_expr(qual, &Type::NoType);
        let qual_ty = qual.ty.clone();
        if self.receiver_has_term(&qual_ty, &name)
            || self.search_extension(&qual_ty, &name, span).is_some()
        {
            return false;
        }
        // A library class the prelude never declares reaches the typer through
        // its pickle, and only `type_select` asks for that. Without asking
        // here, `b ++= xs` on a `scala.collection.mutable.Builder` — whose
        // `+=` / `++=` are `Growable`'s default methods — was rewritten into
        // an assignment and then reported as an unassignable receiver, even
        // though the member is right there.
        if !self.supply_from_pickle(&qual_ty, &name).is_empty() {
            return false;
        }
        // nsc `convertToAssignment`'s `mkUpdate` branch: when the receiver is
        // an indexing, `t(i) op= x` is `t.update(i, t.apply(i) op x)`.
        // Without it `arr(0) += 1` was reported as an unassignable receiver.
        if let TreeKind::Apply {
            fun: callee,
            args: indices,
        } = &qual.kind
        {
            if let Some(table) = index_table(callee) {
                let table = table.clone();
                let indices = indices.clone();
                let rhs_args = args.clone();
                return self
                    .rewrite_update_assignment_op(tree, table, indices, &name, rhs_args, pt);
            }
        }
        if self.is_assignable_lhs(qual) {
            let op = name[..name.len() - 1].to_string();
            let lhs = (**qual).clone();
            let rhs_args = args.clone();
            let plus = Tree {
                id: fun_id,
                span: fun_span,
                kind: TreeKind::Select {
                    qual: Box::new(lhs.clone()),
                    name: op,
                },
                ty: Type::NoType,
                sym: SymbolId::NONE,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            };
            let rhs = Tree {
                id,
                span,
                kind: TreeKind::Apply {
                    fun: Box::new(plus),
                    args: rhs_args,
                },
                ty: Type::NoType,
                sym: SymbolId::NONE,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            };
            tree.kind = TreeKind::Assign {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
            self.type_expr(tree, pt);
            return true;
        }
        if !qual_ty.is_error() {
            // One error, two lines -- what nsc reports here. Raising them as
            // two errors doubled the count and, worse, read as a second,
            // independent failure: `m("a") = 1` on the line above a bad
            // `s -= 2` looked like the thing that "does not convert to
            // assignment", and the `update` desugar was blamed for it.
            self.error(
                span,
                format!(
                    "value {name} is not a member of {}\n  Expression does not convert to \
                     assignment because receiver is not assignable.",
                    self.st.display_type(&qual_ty)
                ),
            );
        }
        tree.ty = Type::Error;
        true
    }

    fn is_assignable_lhs(&self, tree: &Tree) -> bool {
        if tree.sym.is_none() {
            return false;
        }
        let s = self.st.get(tree.sym);
        s.kind == SymKind::Term && s.flags.contains(Flags::MUTABLE)
    }

    /// nsc `convertToAssignment`'s `mkUpdate`: `t(i) op= x` is
    /// `t.update(i, t.apply(i) op x)`. The table and every index are evaluated
    /// once (nsc `gen.evalOnce`), so `f()(g()) += 1` does not run `f` and `g`
    /// twice. The `Assign` case turns the `t(i) = …` back into `t.update`.
    fn rewrite_update_assignment_op(
        &mut self,
        tree: &mut Tree,
        table: Tree,
        indices: Vec<Tree>,
        name: &str,
        rhs_args: Vec<Tree>,
        pt: &Type,
    ) -> bool {
        let span = tree.span;
        let mut stats: Vec<Tree> = Vec::new();
        let table = self.eval_once(table, &mut stats);
        let indices: Vec<Tree> = indices
            .into_iter()
            .map(|i| self.eval_once(i, &mut stats))
            .collect();
        let read = || Tree {
            span,
            ..Tree::dummy(TreeKind::Apply {
                fun: Box::new(table.clone()),
                args: indices.clone(),
            })
        };
        let op = name[..name.len() - 1].to_string();
        let combined = Tree {
            span,
            ..Tree::dummy(TreeKind::Apply {
                fun: Box::new(Tree {
                    span,
                    ..Tree::dummy(TreeKind::Select {
                        qual: Box::new(read()),
                        name: op,
                    })
                }),
                args: rhs_args,
            })
        };
        let assign = TreeKind::Assign {
            lhs: Box::new(read()),
            rhs: Box::new(combined),
        };
        tree.kind = if stats.is_empty() {
            assign
        } else {
            TreeKind::Block {
                stats,
                expr: Box::new(Tree {
                    span,
                    ..Tree::dummy(assign)
                }),
            }
        };
        tree.ty = Type::NoType;
        tree.sym = SymbolId::NONE;
        self.type_expr(tree, pt);
        true
    }

    /// nsc `gen.evalOnce`: hand back a tree that may be used twice. A pure
    /// reference is duplicated; anything else is bound to a fresh local whose
    /// definition is pushed onto `stats`.
    fn eval_once(&mut self, t: Tree, stats: &mut Vec<Tree>) -> Tree {
        // Decide on the *typed* tree — `arr` inside `arr(0)` only looks like a
        // stable reference while it still carries its symbol — then hand back a
        // copy the typer can resolve again from the name.
        let safe = self.is_safe_to_duplicate(&t);
        let mut t = t;
        reset_for_retyping(&mut t);
        if safe {
            return t;
        }
        let name = self.fresh("ev");
        let span = t.span;
        stats.push(Tree {
            span,
            ..Tree::dummy(TreeKind::ValDef {
                mods: scala_rs_parser::Modifiers::default(),
                name: name.clone(),
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(t),
            })
        });
        Tree {
            span,
            ..Tree::dummy(TreeKind::Ident { name })
        }
    }

    /// nsc `treeInfo.isExprSafeToInline`, narrowed to the shapes the update
    /// rewrite duplicates.
    fn is_safe_to_duplicate(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::Literal { .. } | TreeKind::This { .. } | TreeKind::Super { .. } => true,
            TreeKind::Ident { .. } => self.is_stable_ref(t),
            TreeKind::Select { qual, .. } => {
                self.is_stable_ref(t) && self.is_safe_to_duplicate(qual)
            }
            _ => false,
        }
    }

    fn is_stable_ref(&self, t: &Tree) -> bool {
        if t.sym.is_none() {
            return false;
        }
        let s = self.st.get(t.sym);
        matches!(s.kind, SymKind::Term | SymKind::Module)
            && !s.flags.contains(Flags::MUTABLE)
            && !matches!(s.ty, Type::Method { .. })
    }

    pub(crate) fn try_rewrite_dynamic_apply(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let dyn_name = match &tree.kind {
            TreeKind::Apply { fun, .. } => match &fun.kind {
                TreeKind::Select { name, .. }
                    if !matches!(
                        name.as_str(),
                        "applyDynamic" | "selectDynamic" | "updateDynamic" | "applyDynamicNamed"
                    ) =>
                {
                    name.clone()
                }
                _ => return false,
            },
            _ => return false,
        };
        {
            let TreeKind::Apply { fun, .. } = &mut tree.kind else {
                return false;
            };
            let TreeKind::Select { qual, .. } = &mut fun.kind else {
                return false;
            };
            self.type_expr(qual, &Type::NoType);
            if !self.is_dynamic_receiver(&qual.ty) {
                return false;
            }
            if self.receiver_has_term(&qual.ty, &dyn_name) {
                return false;
            }
        }
        if !self.language_dynamics {
            self.dynamics_feature_error(
                tree.span,
                if has_named_dynamic_args(tree) {
                    "applyDynamicNamed"
                } else {
                    "applyDynamic"
                },
            );
            tree.ty = Type::Error;
            return true;
        }
        let span = tree.span;
        let TreeKind::Apply { fun, args } = std::mem::replace(&mut tree.kind, TreeKind::Empty)
        else {
            return false;
        };
        let TreeKind::Select { qual, .. } = fun.kind else {
            tree.kind = TreeKind::Apply { fun, args };
            return false;
        };
        let named = args.iter().any(|a| Self::named_arg_parts(a).is_some());
        let method = if named {
            "applyDynamicNamed"
        } else {
            "applyDynamic"
        };
        let args = if named {
            args.into_iter()
                .map(|a| self.named_dynamic_tuple(a))
                .collect()
        } else {
            args
        };
        let name_lit = Tree::new(
            NodeId(0),
            span,
            TreeKind::Literal {
                lit: Lit::String(dyn_name),
            },
        );
        let sel = Tree {
            id: fun.id,
            span: fun.span,
            kind: TreeKind::Select {
                qual,
                name: method.into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let inner = Tree {
            id: fun.id,
            span: fun.span,
            kind: TreeKind::Apply {
                fun: Box::new(sel),
                args: vec![name_lit],
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        tree.kind = TreeKind::Apply {
            fun: Box::new(inner),
            args,
        };
        self.type_apply(tree, pt);
        true
    }

    fn named_dynamic_tuple(&self, arg: Tree) -> Tree {
        let span = arg.span;
        let (name, value) = if let Some((n, rhs)) = Self::named_arg_parts(&arg) {
            (n, rhs)
        } else {
            (String::new(), arg)
        };
        let name_lit = Tree::new(
            NodeId(0),
            span,
            TreeKind::Literal {
                lit: Lit::String(name),
            },
        );
        let tpt = Tree::new(
            NodeId(0),
            span,
            TreeKind::Ident {
                name: "Tuple2".into(),
            },
        );
        let neu = Tree::new(NodeId(0), span, TreeKind::New { tpt: Box::new(tpt) });
        Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(neu),
                args: vec![name_lit, value],
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        }
    }

    pub(crate) fn try_rewrite_dynamic_update(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        enum DynUpd {
            Select(String),
            Indexed(String),
        }
        let kind = {
            let TreeKind::Assign { lhs, .. } = &mut tree.kind else {
                return false;
            };
            match &mut lhs.kind {
                TreeKind::Select { qual, name }
                    if !matches!(
                        name.as_str(),
                        "updateDynamic" | "selectDynamic" | "applyDynamic" | "applyDynamicNamed"
                    ) =>
                {
                    let dyn_name = name.clone();
                    self.type_expr(qual, &Type::NoType);
                    if !self.is_dynamic_receiver(&qual.ty)
                        || self.receiver_has_term(&qual.ty, &dyn_name)
                    {
                        return false;
                    }
                    DynUpd::Select(dyn_name)
                }
                TreeKind::Apply { fun, .. } => match &mut fun.kind {
                    TreeKind::Select { qual, name }
                        if !matches!(
                            name.as_str(),
                            "update" | "apply" | "updateDynamic" | "selectDynamic"
                        ) =>
                    {
                        let dyn_name = name.clone();
                        self.type_expr(qual, &Type::NoType);
                        if !self.is_dynamic_receiver(&qual.ty)
                            || self.receiver_has_term(&qual.ty, &dyn_name)
                        {
                            return false;
                        }
                        DynUpd::Indexed(dyn_name)
                    }
                    _ => return false,
                },
                _ => return false,
            }
        };
        if !self.language_dynamics {
            let method = match &kind {
                DynUpd::Select(_) => "updateDynamic",
                DynUpd::Indexed(_) => "selectDynamic",
            };
            self.dynamics_feature_error(tree.span, method);
            tree.ty = Type::Error;
            return true;
        }
        let span = tree.span;
        let TreeKind::Assign { lhs, rhs } = std::mem::replace(&mut tree.kind, TreeKind::Empty)
        else {
            return false;
        };
        match kind {
            DynUpd::Select(dyn_name) => {
                let TreeKind::Select { qual, .. } = lhs.kind else {
                    return false;
                };
                let name_lit = Tree::new(
                    NodeId(0),
                    span,
                    TreeKind::Literal {
                        lit: Lit::String(dyn_name),
                    },
                );
                let sel = Tree {
                    id: lhs.id,
                    span: lhs.span,
                    kind: TreeKind::Select {
                        qual,
                        name: "updateDynamic".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                let inner = Tree {
                    id: lhs.id,
                    span: lhs.span,
                    kind: TreeKind::Apply {
                        fun: Box::new(sel),
                        args: vec![name_lit],
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                tree.kind = TreeKind::Apply {
                    fun: Box::new(inner),
                    args: vec![*rhs],
                };
            }
            DynUpd::Indexed(dyn_name) => {
                let TreeKind::Apply { fun, mut args } = lhs.kind else {
                    return false;
                };
                let TreeKind::Select { qual, .. } = fun.kind else {
                    return false;
                };
                args.push(*rhs);
                let name_lit = Tree::new(
                    NodeId(0),
                    span,
                    TreeKind::Literal {
                        lit: Lit::String(dyn_name),
                    },
                );
                let sel = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Select {
                        qual,
                        name: "selectDynamic".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                let selected = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Apply {
                        fun: Box::new(sel),
                        args: vec![name_lit],
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                let update = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Select {
                        qual: Box::new(selected),
                        name: "update".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                tree.kind = TreeKind::Apply {
                    fun: Box::new(update),
                    args,
                };
            }
        }
        self.type_expr(tree, pt);
        true
    }

    /// The primary constructor's parameter symbols, grouped by parameter
    /// list, for a class whose `ctor_fields` were already resolved (`type_class`
    /// syncs the real `<init>` member's own `.paramss` to this shape).
    fn primary_ctor_paramss(&self, class_id: SymbolId) -> Option<Vec<Vec<SymbolId>>> {
        let fields = self.st.get(class_id).ctor_fields.clone();
        self.st
            .get(class_id)
            .members
            .iter()
            .copied()
            .find(|&m| self.st.get(m).name == "<init>" && self.st.get(m).params == fields)
            .map(|m| self.st.get(m).paramss.clone())
    }

    /// `p.copy(x = 1)(y = 2)`: a curried case class's `copy` mirrors the
    /// constructor's own parameter-list shape, same as nsc's synthesized one
    /// does (`TableNode(schemaName, …)(val profileTable: Any)` in slick's
    /// `ast/Node.scala` is the motivating case, via `t.copy(identity =
    /// x)(t.profileTable)`). The single-`Apply` rewrite below only ever sees
    /// one argument list: called on the *inner* `Apply` alone
    /// (`f.copy(a = 2)`, before the outer `Apply` supplying `(f.extra)` is
    /// even considered) it filled every field — including ones that belong
    /// to the second list — from the receiver's own value and returned a
    /// complete instance, so the *outer* `(f.extra)` then read as an
    /// `.apply` call on that instance: "value apply is not a member of
    /// TableNode". This peels the whole `Apply` chain down to `copy` first
    /// and builds one `C(…)(…)` call (the companion's own curried `apply`,
    /// not `new C(…)(…)` -- see the comment below on why) with the matching
    /// list shape, falling through to the single-list rewrite when there is
    /// no second list to peel (the overwhelmingly common case).
    fn try_rewrite_case_copy_curried(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        fn depth_to_copy(t: &Tree) -> Option<u32> {
            match &t.kind {
                TreeKind::Apply { fun, .. } => match &fun.kind {
                    TreeKind::Select { name, .. } | TreeKind::Ident { name } if name == "copy" => {
                        Some(1)
                    }
                    TreeKind::Apply { .. } => depth_to_copy(fun).map(|d| d + 1),
                    _ => None,
                },
                _ => None,
            }
        }
        if depth_to_copy(tree).is_none_or(|d| d < 2) {
            return false;
        }
        // Peel: unwind one `Apply` layer at a time (outermost first),
        // collecting each layer's own argument list, until the innermost
        // `copy` selection/identifier is reached.
        let owned = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        let span = owned.span;
        let mut arg_lists_outer_first: Vec<Vec<Tree>> = Vec::new();
        let mut cur = owned;
        let (qual, is_bare) = loop {
            let TreeKind::Apply { fun, args } = cur.kind else {
                return false;
            };
            arg_lists_outer_first.push(args);
            match fun.kind {
                TreeKind::Apply { .. } => cur = *fun,
                TreeKind::Select { qual, .. } => break (Some(*qual), false),
                TreeKind::Ident { .. } => break (None, true),
                _ => return false,
            }
        };
        let mut arg_lists = arg_lists_outer_first;
        arg_lists.reverse();
        let class_id = if is_bare {
            let cls = self.st.this_class;
            if cls.is_none() {
                return false;
            }
            let here: Vec<SymbolId> = self
                .st
                .get(cls)
                .members
                .iter()
                .copied()
                .filter(|&m| self.st.get(m).name == "copy")
                .collect();
            let resolved = self.st.lookup("copy");
            if here.len() != 1
                || !self.st.get(here[0]).flags.contains(Flags::SYNTHETIC)
                || resolved != here
            {
                return false;
            }
            cls
        } else {
            let mut q = qual.clone().unwrap();
            if q.ty.is_no_type() {
                self.type_expr(&mut q, &Type::NoType);
            }
            match self.st.class_sym_of(&q.ty) {
                Some(c) => c,
                None => return false,
            }
        };
        if !self.st.get(class_id).flags.contains(Flags::CASE) {
            return false;
        }
        if self
            .st
            .lookup_member(class_id, "copy")
            .iter()
            .any(|&s| !self.st.get(s).flags.contains(Flags::SYNTHETIC))
        {
            return false;
        }
        let Some(groups) = self.primary_ctor_paramss(class_id) else {
            return false;
        };
        if groups.len() != arg_lists.len() || groups.len() < 2 {
            return false;
        }
        let qual_expr = if is_bare {
            Tree::dummy(TreeKind::This { qual: None })
        } else {
            qual.unwrap()
        };
        // The receiver is evaluated once, as nsc's `copy$default$n` does.
        let tmp = self.fresh("x$copy");
        let tmp_def = Tree::dummy(TreeKind::ValDef {
            mods: scala_rs_parser::Modifiers::default(),
            name: tmp.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(qual_expr),
        });
        let mut new_arg_lists: Vec<Vec<Tree>> = Vec::with_capacity(groups.len());
        for (group, args) in groups.iter().zip(arg_lists) {
            let names: Vec<String> = group.iter().map(|f| self.st.get(*f).name.clone()).collect();
            let (slots, extra, ok) = self.named_arg_slots(args, &names);
            if ok {
                for a in extra {
                    self.error(a.span, "too many arguments");
                }
            }
            let mut new_args = Vec::with_capacity(slots.len());
            for (i, slot) in slots.into_iter().enumerate() {
                new_args.push(match slot {
                    Some(a) => a,
                    None => Tree::dummy(TreeKind::Select {
                        qual: Box::new(Tree::dummy(TreeKind::Ident { name: tmp.clone() })),
                        name: names[i].clone(),
                    }),
                });
            }
            new_arg_lists.push(new_args);
        }
        // `new C(…)(…)`, which is what nsc's `copy` is. Going through the
        // companion's `apply` instead -- what this did while curried `new` was
        // broken -- is only the same method when the companion is synthetic:
        // slick's `SimpleLiteral` declares its own `apply[T](name)(implicit
        // tpe)`, and a companion that declares any `apply` gets no synthetic
        // one emitted, so `copy()(buildType)` compiled to a call to a method
        // that is not in the classfile.
        let mut ctor = Tree::dummy(TreeKind::New {
            tpt: Box::new(self.resolved_class_tpt(class_id)),
        });
        for args in new_arg_lists {
            ctor = Tree::dummy(TreeKind::Apply {
                fun: Box::new(ctor),
                args,
            });
        }
        *tree = Tree::dummy(TreeKind::Block {
            stats: vec![tmp_def],
            expr: Box::new(ctor),
        });
        tree.span = span;
        self.type_expr(tree, pt);
        true
    }

    /// nsc synthesizes `def copy(x: T = this.x, …): C` on a case class.
    /// Rewriting `p.copy(y = 3)` to a constructor call keeps the omitted
    /// fields coming from the receiver without emitting a synthetic method,
    /// and re-infers the class's type parameters as nsc's `copy[T]` does.
    pub(crate) fn try_rewrite_case_copy(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        if self.try_rewrite_case_copy_curried(tree, pt) {
            return true;
        }
        let shape = match &tree.kind {
            TreeKind::Apply { fun, .. } => match &fun.kind {
                TreeKind::Select { name, .. } if name == "copy" => CopyCallee::Qualified,
                // `copy(from = f2, …)` written inside the case class itself is
                // the same call on `this`. Without this it went through the
                // synthetic `copy` member, whose parameters are the *class's*
                // type parameters: `case class Comprehension[+Fetch <:
                // Option[Node]](…, fetch: Fetch = None, …)` could not be
                // rebuilt with a different `Fetch`, and nsc's `copy[Fetch]`
                // re-infers it.
                TreeKind::Ident { name } if name == "copy" => CopyCallee::Bare,
                _ => return false,
            },
            _ => return false,
        };
        let class_id = match shape {
            CopyCallee::Qualified => {
                let TreeKind::Apply { fun, .. } = &mut tree.kind else {
                    return false;
                };
                let TreeKind::Select { qual, .. } = &mut fun.kind else {
                    return false;
                };
                if qual.ty.is_no_type() {
                    self.type_expr(qual, &Type::NoType);
                }
                match self.st.class_sym_of(&qual.ty) {
                    Some(c) => c,
                    None => return false,
                }
            }
            CopyCallee::Bare => {
                let cls = self.st.this_class;
                if cls.is_none() {
                    return false;
                }
                // Only when the name really does resolve to this class's own
                // synthetic `copy`: a local `def copy`, an import, or an
                // inherited one is an ordinary call.
                let here: Vec<SymbolId> = self
                    .st
                    .get(cls)
                    .members
                    .iter()
                    .copied()
                    .filter(|&m| self.st.get(m).name == "copy")
                    .collect();
                let resolved = self.st.lookup("copy");
                if here.len() != 1
                    || !self.st.get(here[0]).flags.contains(Flags::SYNTHETIC)
                    || resolved != here
                {
                    return false;
                }
                cls
            }
        };
        if !self.st.get(class_id).flags.contains(Flags::CASE) {
            return false;
        }
        {
            let span = tree.span;
            let prefix: Option<&Tree> = match &tree.kind {
                TreeKind::Apply { fun, .. } => match &fun.kind {
                    TreeKind::Select { qual, .. } => Some(qual.as_ref()),
                    _ => None,
                },
                _ => None,
            };
            if self.case_copy_access_error(class_id, prefix, span) {
                tree.ty = Type::Error;
                return true;
            }
        }
        let fields = self.st.get(class_id).ctor_fields.clone();
        if fields.is_empty() {
            return false;
        }
        // A hand-written `copy` wins over the synthetic one.
        if self
            .st
            .lookup_member(class_id, "copy")
            .iter()
            .any(|&s| !self.st.get(s).flags.contains(Flags::SYNTHETIC))
        {
            return false;
        }
        let span = tree.span;
        let (fun, args) = match std::mem::replace(&mut tree.kind, TreeKind::Empty) {
            TreeKind::Apply { fun, args } => (*fun, args),
            _ => return false,
        };
        let qual = match fun.kind {
            TreeKind::Select { qual, .. } => *qual,
            TreeKind::Ident { .. } => Tree::dummy(TreeKind::This { qual: None }),
            _ => return false,
        };
        let names: Vec<String> = fields
            .iter()
            .map(|f| self.st.get(*f).name.clone())
            .collect();
        let (slots, extra, ok) = self.named_arg_slots(args, &names);
        if ok {
            for a in extra {
                self.error(a.span, "too many arguments");
            }
        }
        // The receiver is evaluated once, as nsc's `copy$default$n` does.
        let tmp = self.fresh("x$copy");
        let tmp_def = Tree::dummy(TreeKind::ValDef {
            mods: scala_rs_parser::Modifiers::default(),
            name: tmp.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(qual),
        });
        let mut new_args = Vec::with_capacity(slots.len());
        for (i, slot) in slots.into_iter().enumerate() {
            new_args.push(match slot {
                Some(a) => a,
                None => Tree::dummy(TreeKind::Select {
                    qual: Box::new(Tree::dummy(TreeKind::Ident { name: tmp.clone() })),
                    name: names[i].clone(),
                }),
            });
        }
        let new_tree = Tree::dummy(TreeKind::New {
            tpt: Box::new(self.resolved_class_tpt(class_id)),
        });
        let ctor = Tree::dummy(TreeKind::Apply {
            fun: Box::new(new_tree),
            args: new_args,
        });
        tree.kind = TreeKind::Block {
            stats: vec![tmp_def],
            expr: Box::new(ctor),
        };
        tree.span = span;
        self.type_expr(tree, pt);
        true
    }

    /// Whether some alternative of `fun_ty` can be called with `n` arguments,
    /// counting a repeated parameter and omissible trailing defaults. Types
    /// are not looked at: this is nsc's arity filter, which runs before
    /// alternatives are weighed against the argument types.
    fn some_alt_takes_arity(&self, fun_ty: &Type, fun_sym: SymbolId, n: usize) -> bool {
        let mut sigs: Vec<(SymbolId, Vec<Type>)> = Vec::new();
        match strip_annotations(fun_ty) {
            Type::Overload(alts) => {
                // Prefer the alternatives the selection found: the `Overload`
                // type can carry a subset, and `drop_overridden` removes the
                // ones an override already replaced.
                let named = if fun_sym.is_none() {
                    Vec::new()
                } else {
                    let name = self.st.get(fun_sym).name.clone();
                    self.drop_overridden(self.overload_alternatives(fun_sym, &name))
                };
                if named.is_empty() {
                    for a in alts {
                        if let Type::Method { paramss, .. } = a {
                            sigs.push((
                                SymbolId::NONE,
                                paramss.first().cloned().unwrap_or_default(),
                            ));
                        }
                    }
                } else {
                    for m in named {
                        if let Type::Method { paramss, .. } = &self.st.get(m).ty {
                            sigs.push((m, paramss.first().cloned().unwrap_or_default()));
                        }
                    }
                }
            }
            Type::Class { sym, .. } | Type::ModuleRef(sym) => {
                for m in self.st.lookup_member(*sym, "apply") {
                    if let Type::Method { paramss, .. } = &self.st.get(m).ty {
                        sigs.push((m, paramss.first().cloned().unwrap_or_default()));
                    }
                }
            }
            _ => {}
        }
        sigs.iter().any(|(m, ps)| {
            let (fixed, repeated) = split_repeated(ps);
            if repeated.is_some() {
                return n >= fixed.len();
            }
            n == ps.len() || (n < ps.len() && self.trailing_omissible(*m, 0, n, ps.len()))
        })
    }

    /// `c(args)` where `c` has no `apply` but a conversion of it does.
    ///
    /// Rewrites to `c.apply(args)` and re-types. The rewrite is structural, so
    /// the second pass sees a `Select` and takes the ordinary member path --
    /// it cannot come back here and loop.
    pub(crate) fn retry_apply_extension(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let TreeKind::Apply { fun, .. } = &tree.kind else {
            return false;
        };
        // Already `x.apply(…)`: the Select path has had its turn.
        if matches!(&fun.kind, TreeKind::Select { name, .. } if name == "apply") {
            return false;
        }
        let fun_ty = fun.ty.clone();
        let span = fun.span;
        if fun_ty.is_error() || fun_ty.is_no_type() {
            return false;
        }
        if self.search_extension(&fun_ty, "apply", span).is_none() {
            return false;
        }
        let TreeKind::Apply { fun, .. } = &mut tree.kind else {
            return false;
        };
        let old = std::mem::replace(fun.as_mut(), Tree::dummy(TreeKind::Empty));
        **fun = Tree {
            id: old.id,
            span,
            kind: TreeKind::Select {
                qual: Box::new(old),
                name: "apply".to_string(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        self.type_expr(tree, pt);
        true
    }

    /// nsc's tuple adaptation: an argument list that fits no alternative is
    /// retried packed into a single tuple, so `Some(a, b)` means `Some((a,
    /// b))`. slick's generated `TupleSupport` writes `Some((p._1, p._2),
    /// p._3)` at every arity.
    ///
    /// The retry cannot recurse: the new list holds exactly one argument.
    /// When it does not typecheck either, the tree and the diagnostics are put
    /// back so the error still describes what was written. Named arguments are
    /// left alone -- `f(a = 1, b = 2)` is a names/defaults call, not a tuple.
    ///
    /// An overloaded callee is filtered by *arity* first, and nsc reports
    /// against the alternatives that take the written number of arguments
    /// rather than tupling: `c(1, "x")` against `c(String, String)` /
    /// `c((Int, String))` is the type mismatch scalac gives, and `Lit(1, 2,
    /// 3)` against `apply(String, Any, Boolean = …)` / `apply[T](T)(implicit
    /// Tagged[T])` stays a mismatch too. Only when *no* alternative takes
    /// that many arguments -- `println(1, "a")`, whose alternatives are
    /// nullary and unary -- is the whole list packed into one tuple.
    pub(crate) fn retry_tupled_args(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let TreeKind::Apply { fun, args } = &tree.kind else {
            return false;
        };
        if self.tupling
            || args.len() < 2
            || args.len() > MAX_TUPLE_ARITY
            || Self::has_named_arg(args)
        {
            return false;
        }
        if args.iter().any(|a| a.ty.is_error() || a.ty.is_no_type()) {
            return false;
        }
        let overloaded = match &fun.ty {
            Type::Overload(_) => true,
            Type::Class { sym, .. } | Type::ModuleRef(sym) => {
                self.st.lookup_member(*sym, "apply").len() > 1
            }
            _ => false,
        };
        if overloaded && self.some_alt_takes_arity(&fun.ty, fun.sym, args.len()) {
            return false;
        }
        // nsc never tuples a *varargs* call. `tryTupleApply` only runs where
        // the formals and the arguments disagree in number, and a repeated
        // parameter is expanded to the argument count before that comparison,
        // so the two always agree. Without this, `Seq(a, b)` on elements with
        // no common class quietly became a `Seq[(A, B)]`.
        if self.callee_takes_repeated(fun) {
            return false;
        }
        let saved = tree.clone();
        let mark = self.diags.len();
        let TreeKind::Apply { args, .. } = &mut tree.kind else {
            return false;
        };
        self.tupling = true;
        let elems = std::mem::take(args);
        let span = elems[0].span.merge(elems[elems.len() - 1].span);
        let mut fun = Tree::new(
            NodeId(0),
            span,
            TreeKind::Ident {
                name: format!("Tuple{}", elems.len()),
            },
        );
        fun.scala_ref = true;
        args.push(Tree::new(
            NodeId(0),
            span,
            TreeKind::Apply {
                fun: Box::new(fun),
                args: elems,
            },
        ));
        self.type_apply(tree, pt);
        self.tupling = false;
        // A missing implicit leaves a perfectly good type behind, so the
        // diagnostics -- not the type alone -- decide whether the tupled form
        // really worked.
        let complained = self.diags[mark..]
            .iter()
            .any(|d| d.level == scala_rs_span::Level::Error);
        // The arguments were typed once already, so anything this second pass
        // has to say about them is a duplicate either way.
        self.diags.truncate(mark);
        if complained || tree.ty.is_error() || tree.ty.is_no_type() {
            *tree = saved;
            return false;
        }
        true
    }

    /// Whether the callee's first clause ends in a repeated parameter.
    fn callee_takes_repeated(&self, fun: &Tree) -> bool {
        let is_varargs = |t: &Type| {
            matches!(t, Type::Method { paramss, .. }
                if paramss
                    .first()
                    .and_then(|c| c.last())
                    .is_some_and(|p| matches!(p, Type::Repeated(_))))
        };
        match &fun.ty {
            Type::Method { .. } => is_varargs(&fun.ty),
            Type::Class { sym, .. } | Type::ModuleRef(sym) => self
                .st
                .lookup_member(*sym, "apply")
                .into_iter()
                .any(|m| is_varargs(&self.st.get(m).ty)),
            _ => false,
        }
    }
}
