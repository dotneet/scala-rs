#![allow(dead_code)]
//! Classpath completion and binary symbol loading for the typer.
//!
//! This module owns the on-demand bridge from JVM class files and Scala
//! pickles into the symbol table, including the implicit scopes that become
//! available only after that completion.

use crate::check::*;
use crate::implicits::ImplicitSearch;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    pub(crate) fn complete_binary_member(&mut self, owner: SymbolId, name: &str, span: Span) {
        if owner.is_none() || name.is_empty() {
            return;
        }
        let owner = self.as_type_owner(owner);
        if self.st.get(owner).kind == SymKind::Class {
            self.ensure_java_loaded(owner, span);
            let found = self.st.lookup_member(owner, name);
            if !found.is_empty() {
                // A nested Java class usually reaches the table as a *stub*
                // long before anyone writes its name: `Map.entrySet()`'s
                // generic signature mentions `java/util/Map$Entry`, so reading
                // `Map` alone enters an `Entry` with no parents and no type
                // parameters. Returning here on the strength of that stub
                // meant `java/util/Map$Entry.class` was never read, and
                // `java.util.Map.Entry[String, Int]` drew "Entry does not take
                // type parameters". The nested class file is what carries the
                // nested `Signature` (`<K:…;V:…>`), so complete it too.
                for id in found {
                    if self.st.get(id).kind == SymKind::Class {
                        self.ensure_java_loaded(id, span);
                    }
                }
                return;
            }
        } else {
            let found = self.st.lookup_member(owner, name);
            let has_module = found
                .iter()
                .any(|&id| matches!(self.st.get(id).kind, SymKind::Module | SymKind::ModuleClass));
            if let Some(id) = found
                .iter()
                .copied()
                .find(|&id| self.st.get(id).kind == SymKind::Class)
            {
                self.ensure_java_loaded(id, span);
                return;
            }
            if found
                .iter()
                .any(|&id| self.st.get(id).kind == SymKind::Package)
            {
                return;
            }
            // At package scope an existing module is already the complete
            // source-level answer. Its non-`$` classfile may be only Scala's
            // static-forwarder view (`scala.Function.class`); loading that as
            // though it were a missing class companion can replace the
            // module's pickled curried signatures with flattened descriptors.
            // The ambiguity below is specifically a nested-name ambiguity.
            if self.st.get(owner).kind == SymKind::Package && has_module {
                return;
            }
            // A nested companion can be discovered before its class. This is
            // common for a constructor with defaults: `Outer$Nested$.class`
            // is listed in the outer object's `InnerClasses` table and enters
            // the term `Nested`, while `Outer$Nested.class` is still unread.
            // Finding that module is not proof that the class half is loaded;
            // continue through both binary candidates so a type-position use
            // (`new Outer.Nested`) gets the class and its accessors too.
            // Object-only names simply re-visit their already completed `$`
            // candidate, so they retain the same symbol and behavior.
        }
        // Try every candidate, not just the first hit: a case class with a
        // companion (`Const` / `Const$`, or cats-effect's `Errored` /
        // `Errored$`) is two class files under the same simple name, and
        // stopping at the class alone left the module -- the one term
        // position actually wants, and the only one with `apply`/`unapply`
        // -- never installed. `Const(5)` then read as "value apply is not a
        // member of Const" instead of finding the companion's constructor
        // sugar.
        let mut any = false;
        for internal in self.binary_member_candidates(owner, name) {
            if self.load_binary_into(&internal, owner, span, true) {
                any = true;
            }
        }
        if any {
            return;
        }
        if self.st.get(owner).kind == SymKind::Package {
            // `<root>` is allocated with jvm_name `scala/runtime` (prelude). A
            // top-level Java package like `jprot` lives at `jprot/`, not
            // `scala/runtime/jprot/`.
            let pkg_jvm = if owner == self.st.root {
                String::new()
            } else {
                self.st.get(owner).jvm_name.clone()
            };
            let internal = if pkg_jvm.is_empty() {
                name.to_string()
            } else {
                format!("{pkg_jvm}/{name}")
            };
            let prefix = format!("{internal}/");
            if self.binary.has_package_prefix(&prefix) {
                let _ = crate::classpath::ensure_package(&mut self.st, &internal);
                return;
            }
            // `math.Pi` is a member of the package object `scala/math/package$`,
            // which the package itself only gains once that class is read.
            if self.st.lookup_member(owner, name).is_empty() {
                let _ = self.package_object_of(owner, span);
            }
            self.complete_package_object_member(owner, name, span);
        }
    }

    /// A package object's members reach the symbol table through its
    /// *classfile*, and a JVM descriptor cannot say that a parameter clause is
    /// implicit: `scala.reflect.classTag[T](implicit ct: ClassTag[T])` arrived
    /// as an ordinary one-parameter method, so `classTag[Short]` kept a method
    /// type and conformed to nothing. The pickle is the only place the real
    /// signature is written down.
    fn complete_package_object_member(&mut self, pkg: SymbolId, name: &str, span: Span) {
        if !self.library_abi || !self.st.get(pkg).jvm_name.starts_with("scala/") {
            return;
        }
        let Some(po) = self.package_object_of(pkg, span) else {
            return;
        };
        let added = self
            .pickle
            .complete(&mut self.st, &mut self.binary, po, name);
        if added.is_empty() {
            return;
        }
        // The descriptor-derived symbol for the same JVM method is the same
        // member seen through a poorer lens; keeping both would make every
        // call an overload set. Matched on the erased descriptor, so a real
        // overload the pickle did not supply is left in place.
        let replaced: Vec<String> = added
            .iter()
            .map(|&m| self.st.get(m).jvm_name.clone())
            .filter(|d| !d.is_empty())
            .collect();
        let stale: Vec<SymbolId> = self
            .st
            .get(po)
            .members
            .iter()
            .copied()
            .filter(|&m| {
                !added.contains(&m)
                    && self.st.get(m).name == name
                    && replaced.contains(&self.st.get(m).jvm_name)
            })
            .collect();
        for owner in [po, pkg] {
            self.st
                .get_mut(owner)
                .members
                .retain(|m| !stale.contains(m));
        }
        for m in added {
            if !self.st.get(pkg).members.contains(&m) {
                self.st.get_mut(pkg).members.push(m);
            }
        }
    }

    fn binary_member_candidates(&self, owner: SymbolId, name: &str) -> Vec<String> {
        let name = scala_rs_pickle::names::encode_method_name(name);
        let owner_bin = if owner == self.st.root {
            String::new()
        } else {
            self.st.get(owner).jvm_name.clone()
        };
        let kind = self.st.get(owner).kind;
        let mut out = Vec::new();
        let push = |out: &mut Vec<String>, s: String| {
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        };
        match kind {
            SymKind::Package | SymKind::NoSymbol => {
                let base = if owner_bin.is_empty() {
                    name.to_string()
                } else {
                    format!("{owner_bin}/{name}")
                };
                push(&mut out, base.clone());
                push(&mut out, format!("{base}$"));
            }
            _ => {
                let base = owner_bin.trim_end_matches('$').to_string();
                push(&mut out, format!("{base}${name}"));
                push(&mut out, format!("{base}${name}$"));
                if !owner_bin.is_empty() && owner_bin != base {
                    push(&mut out, format!("{owner_bin}${name}"));
                    push(&mut out, format!("{owner_bin}${name}$"));
                }
            }
        }
        out
    }

    /// Give the prelude's `TupleN` classes the `Product` / `Serializable`
    /// parents nsc gives them.
    ///
    /// Both interfaces live in the library jar, which is read on demand, so
    /// they are forced here -- once, before any unit is named, so that every
    /// later `is_sub_type` sees the same hierarchy. Without the jar
    /// (`--no-scala-library`) neither class is found and nothing is linked:
    /// the private runtime's `scala/Tuple2` implements neither, and a parent
    /// the backend cannot back up would be a lie.
    pub(crate) fn link_tuple_products(&mut self) {
        if !self.library_abi {
            return;
        }
        // `ProductN` too: `TupleN extends ProductN[T1, …]`, so an
        // `Option[Product2[Int, String]]` extractor may return `Some((1, "a"))`
        // (`run/unapply`, `pos/unapplySeq`).
        let products: Vec<String> = (1..=crate::check::MAX_TUPLE_ARITY)
            .map(|n| format!("scala/Product{n}"))
            .collect();
        for jvm in ["scala/Product", "java/io/Serializable"]
            .into_iter()
            .chain(products.iter().map(String::as_str))
        {
            let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let owner = crate::classpath::ensure_package(&mut self.st, pkg);
            self.load_binary_into(jvm, owner, Span::new(0, 0), false);
        }
        crate::prelude_genrep::link_tuple_products(&mut self.st);
    }

    /// `object Vector extends IterableFactory[Vector]` — the edge that lets
    /// `IterableFactory.toFactory` see a collection companion as a `Factory`
    /// source, so `xs.to(Vector)` types. `IterableFactory` lives in the jar,
    /// and `install_prelude` runs before the classpath is installed, so this
    /// has to be a separate pass.
    pub(crate) fn link_collection_factories(&mut self) {
        if !self.library_abi {
            return;
        }
        for jvm in crate::prelude_buildfrom::FACTORY_CLASSES {
            let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let owner = crate::classpath::ensure_package(&mut self.st, pkg);
            self.load_binary_into(jvm, owner, Span::new(0, 0), false);
        }
        for jvm in [
            "scala/collection/EvidenceIterableFactory$",
            "scala/collection/SortedMapFactory$",
        ] {
            if let Some(cls) = crate::classpath::find_by_jvm(&self.st, jvm) {
                self.pickle
                    .adopt_binary_class(&mut self.st, &mut self.binary, cls);
                self.pickle
                    .complete_on_class(&mut self.st, &mut self.binary, cls, "toFactory");
            }
        }
        crate::prelude_buildfrom::install(&mut self.st, true);
    }

    /// `java.lang.String` implements `CharSequence`, `Comparable<String>` and
    /// `Serializable`; the prelude declares it with `AnyRef` alone. Unlike the
    /// tuples above these are JDK classes, so this runs in both library modes.
    pub(crate) fn link_string_parents(&mut self) {
        for jvm in crate::prelude_strhier::STRING_PARENTS {
            let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let owner = crate::classpath::ensure_package(&mut self.st, pkg);
            self.load_binary_into(jvm, owner, Span::new(0, 0), false);
        }
        crate::prelude_strhier::link_string_parents(&mut self.st);
    }

    /// Load `<internal>$`, a class's companion object, if it has one on the
    /// classpath and is not already there.
    ///
    /// Only for Scala class files: a *Java* class's `Foo$` is a nested class,
    /// not a companion, and installing it would enter a class called `Foo$` in
    /// the package.
    ///
    /// `scala.*` is *not* excluded, though the prelude is what describes the
    /// standard library. The prelude describes what programs *name*, and
    /// nothing ever names an implicit -- it is found by searching a scope. So
    /// a library companion the prelude never declared held no witnesses at
    /// all. `scala.collection.BuildFrom` is one, and `buildFromIterableOps` is
    /// the only witness `LazyZip2.map` can use: unless the program happened to
    /// *import* `BuildFrom` by name, it was in no scope, which is what made a
    /// fully applied `implicitly[BuildFrom[…]]` work while
    /// `xs.lazyZip(ys).map(f)` did not.
    ///
    /// Nothing a hand-written declaration owns is replaced: a class that
    /// already has a companion returns at the check above, a companion already
    /// entered under that JVM name is left alone, and for `scala.*` only the
    /// implicits are installed -- everything else keeps coming from the pickle
    /// on demand, exactly as it did when the companion was an empty stub.
    pub(crate) fn load_companion_module(&mut self, class_id: SymbolId) {
        if class_id.is_none() || self.st.get(class_id).kind != SymKind::Class {
            return;
        }
        if self.st.companion_module(class_id).is_some() {
            return;
        }
        // Not gated on the `JAVA` flag: `find_or_stub_java_class` sets it on
        // every placeholder it enters, including Scala classes reached through
        // a parent list, and nothing clears it afterwards. The classfile's own
        // `is_scala` below is the honest test.
        let internal = self.st.get(class_id).jvm_name.clone();
        if internal.is_empty()
            || internal.ends_with('$')
            || internal.starts_with('[')
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
        {
            return;
        }
        // `materialize::ensure_tag_module` hand-builds `TypeTags#TypeTag$` and
        // `TypeTags#WeakTypeTag$` -- `apply(mirror, creator)` is a signature no
        // class file carries -- and takes the presence of a symbol under that
        // JVM name as part of the record that it did. Entering the bare class
        // file first made it stand down, and `typeOf[T]`'s
        // `TypeTag.apply(mirror, creator)` had no `apply` to resolve to
        // (slick's `ShapedValue` / `TableQuery` macros). Those two are the
        // whole exception. Until 2026-09-13 this returned for *every* nested
        // `scala.*` class, which left `scala.reflect.api.Printers.BooleanFlag`
        // -- a nested case class whose companion carries the only
        // `Boolean => BooleanFlag` conversion there is -- with an empty
        // implicit scope, so `showRaw(tree, printIds = true)` could not be
        // typed at all (12 `run` tests of the corpus).
        if matches!(
            internal.as_str(),
            crate::materialize::TYPE_TAG | crate::materialize::WEAK_TYPE_TAG
        ) {
            return;
        }
        let module = format!("{internal}$");
        // Already there under that JVM name; never enter a second copy.
        if crate::classpath::find_by_jvm(&self.st, &module).is_some() {
            return;
        }
        if !self.completed_java.insert(module.clone()) {
            return;
        }
        let Ok(Some(bytes)) = self.binary.find_class(&module) else {
            return;
        };
        let Ok(jc) = crate::javaclass::parse_java_classfile(&bytes) else {
            return;
        };
        if !jc.is_scala {
            return;
        }
        // A *nested* class's companion belongs to whatever encloses the class,
        // not to the package. `companion_module` looks for a module of the
        // same name among the class's own owner's members, and
        // `cats/effect/kernel/Ref$Make$` installed in the package
        // `cats.effect.kernel` under the name `Make` was invisible from the
        // trait `Ref.Make`, whose owner is `Ref`. `Ref.of`'s
        // `implicit mk: Make[F]` then searched an empty implicit scope --
        // unless the program happened to *write* `Ref.Make` somewhere, which
        // builds the companion by another route and made the failure look
        // order-dependent (slick's `ConcurrencyControl.scala`).
        let owner = {
            let o = self.st.get(class_id).owner;
            if !o.is_none() && self.st.get(o).is_class_like() {
                o
            } else if let Some(encl) = internal
                .rsplit_once('$')
                .and_then(|(p, _)| crate::classpath::find_by_jvm(&self.st, p))
                .filter(|&e| self.st.get(e).is_class_like())
            {
                // The class itself reached the symbol table flattened, from a
                // JVM descriptor, so its own `owner` is the *package*. The
                // companion of an inner class still belongs to the class that
                // encloses it: installed in the package it would look like a
                // static object, and the call to one of its members was emitted
                // as `Printers$BooleanFlag$.MODULE$`, a field an inner object
                // does not have.
                encl
            } else {
                let pkg = internal.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                crate::classpath::ensure_package(&mut self.st, pkg)
            }
        };
        let mid = crate::classpath::install_java_class_in(&mut self.st, &jc, owner);
        // SLS 7.2 names the companion *object*, whose members include the ones
        // it inherits, and 2.13 puts the low-priority half of an implicit set
        // in traits the object mixes in:
        // `object BuildFrom extends BuildFromLowPriority1 extends
        // BuildFromLowPriority2`, and `buildFromIterableOps` -- the only
        // witness a plain `List` receiver has -- is declared in the *last* of
        // those. Loading only the object left it invisible.
        self.complete_java_parents(mid, Span::new(0, 0));
        // Deliberately *not* `adopt_binary_class`: only the implicits are
        // wanted here, and adopting the whole companion costs minutes.
        // For a standard-library companion the *pickle* is the authority, and
        // the ordinary on-demand path reads it a name at a time. The members
        // the classfile reader just entered carry erased signatures and would
        // sit next to the pickled ones as bogus overloads -- `Option$` gained
        // a second `apply` and `Option(2)` became `ambiguous overload`. Before
        // this pass existed, `scala.*` companions reached the typer as an
        // empty stub and got everything from the pickle; keep it that way and
        // only add what a stub cannot have: the implicits, below.
        if internal.starts_with("scala/") {
            self.st.get_mut(mid).members.clear();
        }
        let mut work = vec![mid];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = work.pop() {
            if id.is_none() || !seen.insert(id.0) {
                continue;
            }
            self.pickle
                .supply_implicit_members(&mut self.st, &mut self.binary, id);
            for p in self.st.get(id).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                }
            }
        }
    }

    /// Apply the implicit clauses of an implicit conversion that has any.
    ///
    /// `tree` is the conversion already applied to the receiver; a conversion
    /// like cats' `toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])`
    /// needs a second application for its implicit clause, or codegen emits a
    /// call one argument short of the descriptor.
    pub(crate) fn fill_conv_implicits(
        &mut self,
        conv: SymbolId,
        from: &Type,
        mut tree: Tree,
        span: Span,
    ) -> Tree {
        for clause in self.conv_implicit_params(conv, from, &tree.ty) {
            let mut args = Vec::with_capacity(clause.len());
            for want in &clause {
                if let Type::Class { sym, args } = want {
                    if self.st.get(*sym).jvm_name == "scala/reflect/ClassTag" {
                        if let Some(Type::TypeParam(tp)) = args.first() {
                            if self.st.get(conv).tparams.contains(tp) {
                                self.error(
                                    span,
                                    format!(
                                        "type {} is an unresolved spliceable type",
                                        self.st.get(*tp).name
                                    ),
                                );
                                return tree;
                            }
                        }
                    }
                }
                self.warm_implicit_scope(want);
                let mut search = self.search_implicit(want);
                // The same completion `fill_implicit_params_in` does for an
                // ordinary implicit clause (`agent/tail6`): a candidate whose
                // class came from a jar answers for its supertypes only once
                // something has read its parents, and the search itself runs
                // under an immutable borrow. `implicit val asyncF: Async[F]`
                // in a trait could not answer `toFlatMapOps`'s `FlatMap[F]`
                // until some earlier line in the same file happened to warm
                // it (slick's `BasicBackend.scala`, `run`).
                if matches!(search, ImplicitSearch::None)
                    && self.warm_implicit_candidates(std::slice::from_ref(want))
                {
                    search = self.search_implicit(want);
                }
                match search {
                    ImplicitSearch::Found(id) => {
                        let mut a = self.implicit_tree(id, want, span, 0);
                        self.adapt(&mut a, want);
                        args.push(a);
                    }
                    ImplicitSearch::None if self.classtag_apply_fallback(want, span).is_some() => {
                        args.push(self.classtag_apply_fallback(want, span).unwrap());
                    }
                    _ => {
                        // SLS 7.2: an implicit parameter of type `A => B` is a
                        // view request, and an `implicit def` answers it
                        // eta-expanded -- the same fallback
                        // `fill_implicit_params_in` makes for a method's own
                        // clause. An *inserted conversion*'s clause never got
                        // it, so a rule whose parameter is a view could not be
                        // applied: slick's `Ordered.tuple2Ordered(t)(ev1, ev2)`
                        // wants `ev2: Rep[Int] => Ordered`, which is
                        // `columnToOrdered`, and gitbucket's
                        // `sortBy { … => issue.issueId.desc -> commentId }` had
                        // no `Ordered` for its pair.
                        if let Some(lam) = self
                            .identity_view(want, span)
                            .or_else(|| self.conversion_view(want, span))
                        {
                            args.push(lam);
                            continue;
                        }
                        let diverged = self.diverged_implicit.borrow().clone();
                        self.error(span, self.missing_implicit_message(want, diverged));
                        return tree;
                    }
                }
            }
            let ty = tree.ty.clone();
            tree = Tree {
                id: NodeId(0),
                span,
                kind: TreeKind::Apply {
                    fun: Box::new(tree),
                    args,
                },
                ty,
                sym: conv,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
                byname_thunk: false,
                byname_type_marker: false,
            };
        }
        // This application was synthesized rather than passed through
        // type_apply. Its escaping parameters still belong to the caller's
        // inference problem, just like a polymorphic receiver written out.
        let open = self.undetermined_of(&tree);
        self.undet_tvars.extend(open);
        tree
    }

    /// Make sure every companion object in `pt`'s implicit scope is loaded.
    ///
    /// A companion that came from a jar is a class file of its own that nothing
    /// else asks for, so without this `Async[IO]` searches an implicit scope
    /// that does not yet contain `cats.effect.IO.asyncForIO` — the only place
    /// that witness exists. The search runs under an immutable borrow and
    /// cannot load anything itself, so it has to happen here, on demand: doing
    /// it for every jar class as it is adopted pulls in the whole transitive
    /// closure of cats-effect and takes minutes.
    pub(crate) fn warm_implicit_scope(&mut self, pt: &Type) {
        self.warm_implicit_scope_once(pt);
    }

    /// The same, for the *candidates* rather than the wanted type.
    ///
    /// A candidate fits a supertype of its own declared type, and for a class
    /// that came from a jar those parents are only read when something warms
    /// it. `class C[F[_]](implicit F: Async[F])` asking for `Sync[F]` -- or,
    /// through `cats.effect.syntax`, `GenTemporal[F, E]` -- searched with
    /// `Async`'s parent list still empty and found nothing; asking for
    /// `Async[F]` first anywhere in the same file made the later searches
    /// work, which is the shape of a missing completion, not of a scoping
    /// rule. Reported by `slick/basic/ConcurrencyControl.scala`.
    ///
    /// Run only after a search has already come up empty: it reads a pickle
    /// per candidate class, and `warmed_scopes` makes each one a one-off, but
    /// the walk itself is not free. Answers whether anything was new, so the
    /// caller only retries the search when a retry could say something else.
    ///
    /// `wanted` are the types the search came up empty on. Their classes need
    /// their parents just as much: `candidate_bounds_hold` asks whether the
    /// solution for a candidate's `Level <: ShapeLevel` is one, and slick's
    /// `Query.map` wants a `Shape[_ <: FlatShapeLevel, …]`, so the answer is
    /// read off `FlatShapeLevel`'s parents -- a jar class the program never
    /// names. Empty, that said no, and `q.map(_.title)` was "could not find
    /// implicit value of type Shape[_ <: FlatShapeLevel, Rep[String], T, G]"
    /// while naming `FlatShapeLevel` anywhere in the same file fixed it.
    pub(crate) fn warm_implicit_candidates(&mut self, wanted: &[Type]) -> bool {
        let mut completed = self.warm_inherited_implicit_members(wanted);
        let mut cands = self.implicits_in_scope();
        // The companion candidates too. `search_implicit_uncached` falls back
        // to `companion_implicits(pt)` when nothing lexical fits, so those are
        // real candidates and their result types need parents just as much --
        // and they are the ones the program is *least* likely to have named,
        // since the whole point of SLS 7.2 is that they need no import.
        // `TypedType.typedTypeToOptionTypedType[T]: OptionTypedType[T]` is the
        // case: `OptionTypedType extends TypedType[Option[T]]` is the only
        // thing that makes it fit `TypedType[Option[String]]`, and with its
        // parent list still empty `plausibly_inhabits` rejected it before a
        // single unification ran. Every `column[Option[T]]` in slick was
        // "could not find implicit value of type TypedType[Option[String]]",
        // and writing `OptionTypedType` anywhere in the same file fixed it.
        for w in wanted {
            cands.extend(self.companion_implicits(w));
        }
        cands.sort_unstable_by_key(|id| id.0);
        cands.dedup_by_key(|id| id.0);
        for &id in &cands {
            let before = self.st.get(id).ty.clone();
            let unknown = match &before {
                Type::Method { ret, .. } => ret.is_no_type(),
                ty => ty.is_no_type(),
            };
            if unknown && !self.lazy_completing.contains(&id) {
                self.complete_lazy_sig(id, Span::DUMMY);
                completed |= self.st.get(id).ty != before;
            }
        }
        let instance_depth = wanted
            .iter()
            .map(crate::implicits::complexity)
            .max()
            .unwrap_or(0)
            .max(crate::implicits::MAX_IMPLICIT_DEPTH);
        for &id in &cands {
            completed |= self.prepare_implicit_instances(id, instance_depth);
        }
        // A derivation candidate can ask for a witness whose companion the
        // source never names. Cats' `Isomorphisms.invariant[F]`, for example,
        // returns the wanted `Isomorphisms[Option]` but first needs an
        // `Invariant[Option]`; the latter lives in `Option`'s companion. The
        // immutable search cannot load that companion while trying the
        // candidate, so warm just the clauses of candidates whose result can
        // plausibly inhabit one of the wanted types. Warming every imported
        // candidate here is both expensive and risks completing unrelated
        // standard-library hierarchies.
        let mut nested = Vec::new();
        for &id in &cands {
            if !self.only_implicit_clauses(id) {
                continue;
            }
            if let Type::Method { paramss, ret } = &*self.implicit_candidate_ty(id) {
                // This recovery is for typeclass-shaped results. For a
                // non-class target `plausibly_inhabits` deliberately answers
                // conservatively, which made nearly every imported implicit
                // look relevant and warmed unrelated reflection hierarchies
                // while compiling Cats/Slick macros.
                if !wanted.iter().any(|w| {
                    matches!((&**ret, w), (Type::Class { .. }, Type::Class { .. }))
                        && self.plausibly_inhabits(ret, w)
                }) {
                    continue;
                }
                for p in paramss.iter().flatten() {
                    if !nested.contains(p) {
                        nested.push(p.clone());
                    }
                }
            }
        }
        for n in nested {
            completed |= self.warm_implicit_scope_once(&n);
        }
        let tys: Vec<Type> = cands
            .into_iter()
            .map(|id| self.implicit_candidate_ty(id).into_owned())
            .chain(wanted.iter().cloned())
            .collect();
        let mut fresh = completed;
        for t in tys {
            // Parents only. Warming a candidate's *implicit scope* the way
            // `warm_implicit_scope` warms the wanted type's pulls pickled
            // parents onto standard-library companions -- the hazard
            // `warm_own_scope_once` documents -- and cost slick two new
            // `containsSymbol(Set[A])` overload errors when it was tried.
            fresh |= self.ensure_pickled_parents(&t);
        }
        fresh
    }

    /// Load implicit declarations inherited by the class currently being
    /// typed. They are lexical candidates, but a jar parent exposes them only
    /// through its pickle and immutable implicit search cannot complete that
    /// pickle itself. MUnit's `ScalaCheckSuite.unitToProp` is the concrete
    /// case: Cats' `forAll { ... assert(...) }` needs it as `Unit => Prop`.
    ///
    /// This runs only after a search failed. `supply_implicit_members` is
    /// cached per class and installs just implicit declarations, so walking a
    /// deep framework hierarchy does not adopt every ordinary API member.
    fn warm_inherited_implicit_members(&mut self, _wanted: &[Type]) -> bool {
        if !self.library_abi || self.st.this_class.is_none() {
            return false;
        }
        // The only missing inherited candidate observed on real test-suite
        // sources is MUnit ScalaCheckSuite's `unitToProp`. Loading every
        // inherited implicit after every failed search also exposes Scalatra's
        // `request2Session` beside a controller-local `session`, and eagerly
        // changes reflection/quasiquote searches while compiling Cats and
        // Slick themselves. First prove the current suite inherits that exact
        // trait, then complete its inherited implicit environment.
        let mut work = vec![self.st.this_class];
        let mut seen = rustc_hash::FxHashSet::default();
        let mut hierarchy = Vec::new();
        let mut found_scalacheck_suite = false;
        let mut fresh_parents = false;
        while let Some(c) = work.pop() {
            if c.is_none() || !seen.insert(c.0) {
                continue;
            }
            hierarchy.push(c);
            if c.0 >= self.st.prelude_end && !self.st.is_source_class(c) {
                let ty = self.st.type_of_class(c);
                fresh_parents |= self.ensure_pickled_parents(&ty);
                found_scalacheck_suite |= self.st.get(c).jvm_name == "munit/ScalaCheckSuite";
            }
            for p in self.st.get(c).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                }
            }
        }
        if !found_scalacheck_suite {
            return false;
        }
        let mut fresh = fresh_parents;
        for c in hierarchy {
            if c.0 >= self.st.prelude_end && !self.st.is_source_class(c) {
                fresh |= self
                    .pickle
                    .supply_implicit_members(&mut self.st, &mut self.binary, c)
                    != 0;
            }
        }
        fresh
    }

    /// Attach the pickled parents of every class `ty` names, and of the
    /// parents that appear as that goes on.
    ///
    /// `PickleSupply::attach_parents` runs one level at a time and only for a
    /// class something has completed a member of. A candidate the program
    /// merely *named* -- `(implicit F: Async[F])` -- has an empty parent list
    /// until then, so it fits nothing but its own type. Answers whether any
    /// class gained parents.
    fn ensure_pickled_parents(&mut self, ty: &Type) -> bool {
        if !self.library_abi {
            return false;
        }
        let mut work: Vec<SymbolId> = self.implicit_scope_classes(ty);
        let mut seen: std::collections::HashSet<u32> = work.iter().map(|c| c.0).collect();
        let mut fresh = false;
        while let Some(c) = work.pop() {
            if c.is_none() {
                continue;
            }
            // Never the standard library. Its hierarchy is the prelude's,
            // hand-written and reasoned about, and topping it up from the
            // class files rewrote `mutable.HashSet`'s parents well enough to
            // turn `HashSet[A]`'s `+`/`contains` into `Set`'s -- two new
            // errors in slick for a hierarchy nobody had asked to change.
            // What this is for is a *jar* class the program only named.
            let jvm = self.st.get(c).jvm_name.clone();
            if c.0 < self.st.prelude_end || jvm.starts_with("scala/") || jvm.starts_with("java/") {
                continue;
            }
            let before = self.st.get(c).parents.len();
            // Only parents are needed for a module. Whole-module adoption
            // can replace the implicit declarations just published by its
            // companion loader, including nested implicit object identities.
            if !matches!(self.st.get(c).kind, SymKind::Module | SymKind::ModuleClass) {
                self.ensure_java_loaded(c, Span::DUMMY);
            }
            self.pickle
                .ensure_parents(&mut self.st, &mut self.binary, c);
            fresh |= self.st.get(c).parents.len() != before;
            for p in self.st.get(c).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    if seen.insert(ps.0) {
                        work.push(ps);
                    }
                }
            }
        }
        fresh
    }

    /// [`Self::warm_implicit_scope`], reporting whether any class in `pt`'s
    /// implicit scope had not been warmed before. Callers that only want to
    /// *retry* something after new implicits appeared can skip the retry when
    /// this says nothing is new.
    pub(crate) fn warm_implicit_scope_once(&mut self, pt: &Type) -> bool {
        let mut fresh = false;
        for c in self.implicit_scope_classes(pt) {
            fresh |= self.warm_one_scope(c);
        }
        fresh
    }

    /// Warm only the class `ty` *names*, not the base classes SLS 7.2 adds to
    /// its implicit scope.
    ///
    /// Reading a companion's pickle attaches that companion's own pickled
    /// parents, and for a collection those are the factory traits the prelude
    /// models by hand: warming `mutable.Set[T]`'s full scope reached
    /// `collection.Iterable` and gave `Iterable$` (and `Seq$`, `Set$`, …) a
    /// pickled `IterableFactory.Delegate` parent, whose `apply[A](A*): CC[A]`
    /// then stood next to the prelude's own — and `mutable.Set[TypeSymbol]()`
    /// came back as `Set[A]`. The conversions this is here to find
    /// (`Option.option2Iterable`) live on the companion of the type itself, so
    /// nothing is lost by stopping there.
    /// Read the classfile behind each argument's class, so `is_sub_type` can
    /// see its parents. Answers whether any of them had not been read yet.
    ///
    /// `find_or_stub_java_class` enters a class named by a descriptor with
    /// `parents = [AnyRef]` and nothing else; until the classfile itself is
    /// read, that stub conforms to nothing. Overload scoring runs on `&self`
    /// and cannot read one, so the callers that fail ask for this and score
    /// again.
    pub(crate) fn warm_java_args(&mut self, arg_tys: &[Type]) -> bool {
        let classes: Vec<SymbolId> = arg_tys
            .iter()
            .filter_map(|t| self.st.class_sym_of(t))
            .collect();
        let mut fresh = false;
        for c in classes {
            let jvm = self.st.get(c).jvm_name.clone();
            if jvm.is_empty() || self.completed_java.contains(&jvm) {
                continue;
            }
            self.ensure_java_loaded(c, Span::DUMMY);
            fresh = true;
        }
        fresh
    }

    /// Force the class files behind an argument's type before the call is
    /// resolved against it.
    ///
    /// A `-cp` class the source never *names* has nothing to complete it:
    /// `ensure_class` leaves a `JAVA`-flagged placeholder for the ordinary
    /// loader, and the loader only runs where the program mentions the class.
    /// scalatra-forms' `mapping(...)` returns a `MappingValueType[T]`, which
    /// gitbucket only ever infers, so the class kept the empty `AnyRef` parent
    /// list and nothing knew it `implements ValueType[T]` -- the parameter type
    /// of the `post[T](path, form: ValueType[T])(action: T => Any)` it was
    /// being passed to. nsc has no such gap: a symbol's info is completed the
    /// first time anything asks for it, and `isApplicable` asks.
    ///
    /// Once per class, and only in library mode, so a call in a loop pays for
    /// nothing.
    pub(crate) fn complete_arg_classes(&mut self, tys: &[Type]) -> bool {
        if !self.library_abi {
            return false;
        }
        let mut fresh = false;
        for t in tys {
            let Some(c) = self.st.class_sym_of(t) else {
                continue;
            };
            if c.0 < self.st.prelude_end || !self.completed_arg_classes.insert(c.0) {
                continue;
            }
            fresh |= self.ensure_pickled_parents(t);
        }
        fresh
    }

    pub(crate) fn warm_own_scope_once(&mut self, ty: &Type) -> bool {
        match self.st.class_sym_of(ty) {
            Some(c) => self.warm_one_scope(c),
            None => false,
        }
    }

    /// The classes of every parameter of every alternative of an overloaded
    /// reference. Used to warm their implicit scopes before a second
    /// applicability pass: a view that makes an argument fit is normally
    /// declared by the companion of the *parameter* type.
    pub(crate) fn overload_param_classes(&self, fun_ty: &Type) -> Vec<SymbolId> {
        let alts: &[Type] = match fun_ty {
            Type::Overload(alts) => alts,
            other => std::slice::from_ref(other),
        };
        let mut out = Vec::new();
        for alt in alts {
            if let Type::Method { paramss, .. } = alt {
                for clause in paramss {
                    for p in clause {
                        if let Some(c) = self.st.class_sym_of(p) {
                            if !out.contains(&c) {
                                out.push(c);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub(crate) fn warm_one_scope_pub(&mut self, c: SymbolId) -> bool {
        self.warm_one_scope(c)
    }

    fn warm_one_scope(&mut self, c: SymbolId) -> bool {
        if c.is_none() || !self.warmed_scopes.insert(c.0) {
            return false;
        }
        self.load_companion_module(c);
        self.warm_pickled_implicits(c);
        if self.st.get(c).kind == SymKind::ModuleClass
            && self.st.get(c).jvm_name.starts_with("scala/collection/")
        {
            crate::prelude_buildfrom::link_evidence_factories(&mut self.st);
        }
        true
    }

    /// The implicit members a *standard library* companion declares.
    ///
    /// [`Self::load_companion_module`] deliberately stops at `scala.*`: the
    /// prelude is what describes the library. But the prelude describes what
    /// programs *name*, and nothing ever names an implicit -- it is found by
    /// searching a scope. `Option.option2Iterable` was therefore in no
    /// member list at all, and `where.reduceLeft(f)` / `c.where.toSeq` on an
    /// `Option[Node]` (slick's `JdbcStatementBuilderComponent`) were
    /// `value reduceLeft is not a member of Option[Node]`.
    ///
    /// Only a name the companion has no member for is asked, so a
    /// hand-written prelude declaration still wins and no second copy of one
    /// is installed next to it.
    pub(crate) fn warm_binary_implicit_result_parents(&mut self, owner: SymbolId) {
        if owner.0 < self.st.prelude_end
            || self.st.is_source_class(owner)
            || (!self.st.pending_classpath_signatures.contains(&owner)
                && !self.pickle.pickle_readable(&self.st, owner))
        {
            return;
        }
        // A lazy companion can return a class eagerly found in a directory.
        // Restore that result hierarchy before an erased parent makes an
        // inapplicable candidate look like an ambiguous or successful match.
        let results: Vec<Type> = self
            .st
            .get(owner)
            .members
            .iter()
            .filter_map(|&id| {
                let s = self.st.get(id);
                if !s.flags.contains(Flags::IMPLICIT) {
                    return None;
                }
                let mut result = &s.ty;
                while let Type::Method { ret, .. } = result {
                    result = ret;
                }
                Some(result.clone())
            })
            .collect();
        for result in results {
            self.ensure_pickled_parents(&result);
        }
    }

    fn warm_pickled_implicits(&mut self, class_id: SymbolId) {
        if !self.library_abi || class_id.is_none() {
            return;
        }
        // `object Int`'s implicits are `int2long` / `int2float` / `int2double`
        // -- the numeric widenings, which `weak_conforms` already implements
        // directly. As *views* they would only compete: `n + ":"` has no
        // `Int#+(String)` in the prelude, so it is `any2stringadd`, and with
        // three more conversions that also offer a `+` the search became
        // ambiguous and the selection failed outright.
        if self.st.is_primitive_value_class(class_id) {
            return;
        }
        let mcls = match self.st.get(class_id).kind {
            SymKind::Module => self.st.module_class_of(class_id),
            SymKind::ModuleClass => class_id,
            _ => self.st.companion_module_class_for_implicits(class_id),
        };
        if mcls.is_none() {
            return;
        }
        // The companion object's *own* declarations, and the ones it inherits.
        // SLS 7.2 names the object, and an object's members include inherited
        // ones: `object Ordering extends LowPriorityOrderingImplicits`, and
        // `ordered[A](implicit asComparable: A => Comparable[A])` -- the only
        // way to an `Ordering[Null]` (slick's `ScalaBaseType.nullType`) -- is
        // declared by that parent trait, whose member list stays empty until
        // something completes it. The parent is only asked for the members it
        // does not have; nothing about the hierarchy itself is touched.
        let mut work = vec![mcls];
        let mut walked = std::collections::HashSet::new();
        while let Some(c) = work.pop() {
            if c.is_none() || !walked.insert(c.0) {
                continue;
            }
            // Directory discovery can have installed the companion already.
            // Read only its implicit declarations, just as lazy jar discovery
            // does, without adopting an entire module beside existing members.
            if self.st.pending_classpath_signatures.contains(&c) && !self.st.is_source_class(c) {
                self.pickle
                    .supply_implicit_members(&mut self.st, &mut self.binary, c);
            }
            for n in self
                .pickle
                .implicit_member_names(&self.st, &mut self.binary, c)
            {
                if !self
                    .st
                    .lookup_member(c, &n)
                    .iter()
                    .any(|&id| self.st.get(id).flags.contains(Flags::IMPLICIT))
                {
                    self.supply_from_pickle_class(c, &n);
                }
            }
            self.warm_binary_implicit_result_parents(c);
            for p in self.st.get(c).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                }
            }
        }
    }

    pub(crate) fn load_binary_into(
        &mut self,
        internal: &str,
        owner: SymbolId,
        span: Span,
        with_nested: bool,
    ) -> bool {
        if internal.is_empty() {
            return false;
        }
        if !self.completed_java.insert(internal.to_string()) {
            let Some(id) = crate::classpath::find_by_jvm(&self.st, internal) else {
                return false;
            };
            // Read once, but not necessarily *for this owner*: see
            // `classpath::enter_loaded_in_owner`.
            crate::classpath::enter_loaded_in_owner(&mut self.st, id, owner);
            return true;
        }
        match self.binary.find_class(internal) {
            Ok(Some(bytes)) => match crate::javaclass::parse_java_classfile(&bytes) {
                Ok(jc) => {
                    let id = crate::classpath::install_java_class_in(&mut self.st, &jc, owner);
                    // A Scala classfile on `-cp` carries a `ScalaSignature`,
                    // and that is the only place its higher kinds are written
                    // down. Read them off it before anything looks at the
                    // symbol; the classfile reader's view stays underneath for
                    // whatever the pickle cannot express.
                    if jc.is_scala {
                        self.pickle
                            .adopt_binary_class(&mut self.st, &mut self.binary, id);
                    }
                    self.complete_java_parents(id, span);
                    if with_nested {
                        self.complete_scala_nested(id, &jc, span);
                    }
                    true
                }
                Err(e) => {
                    self.error(
                        span,
                        format!("unsupported classfile {}: {e}", internal.replace('/', ".")),
                    );
                    false
                }
            },
            Ok(None) => false,
            Err(e) => {
                self.error(
                    span,
                    format!("unsupported classfile {}: {e}", internal.replace('/', ".")),
                );
                false
            }
        }
    }

    fn complete_scala_nested(
        &mut self,
        class_id: SymbolId,
        jc: &crate::javaclass::JavaClass,
        span: Span,
    ) {
        if !jc.is_scala {
            return;
        }
        let jvm = jc.internal_name.clone();
        if jvm.trim_end_matches('$').contains('$') {
            return;
        }
        let pkg_owner = self.st.get(class_id).owner;
        let companion = self.ensure_scala_companion(class_id, pkg_owner, span);
        let nest_owner = if companion.is_none() {
            class_id
        } else {
            self.st.module_class_of(companion)
        };
        let outer_trait = if matches!(
            self.st.get(class_id).kind,
            SymKind::Module | SymKind::ModuleClass
        ) {
            let stripped = jvm.trim_end_matches('$').to_string();
            crate::classpath::find_by_jvm(&self.st, &stripped).unwrap_or(class_id)
        } else {
            class_id
        };
        // `InnerClasses` records every nested class the file *mentions*, not
        // only the ones it declares: `cats/effect/kernel/MonadCancel.class`
        // lists `cats/syntax/package$all$`. Adopting those installed
        // `cats.syntax.all` as a member of `MonadCancel`, and since
        // `load_binary_into` completes a class file once, the later
        // `import cats.syntax.all._` found nothing to load and nothing in the
        // package object -- but only when something under `cats.effect` was
        // imported first, which is why it looked like an ordering quirk.
        let nest_prefix = format!("{}$", jvm.trim_end_matches('$'));
        for inner in &jc.inner_classes {
            if !inner.inner_jvm.ends_with('$') || inner.inner_jvm.contains("$anon") {
                continue;
            }
            if !inner.inner_jvm.starts_with(&nest_prefix) {
                continue;
            }
            let simple = crate::classpath::java_simple_name(&inner.inner_jvm);
            if simple.is_empty() {
                continue;
            }
            // Check declarations, not inherited lookup. Every module class
            // inherits `AnyRef.eq`; treating that method as an existing
            // declaration made us skip the real nested `object eq` from a
            // package object's `InnerClasses` table. Cats then exposed
            // `cats.syntax.eq` only as the inherited Boolean method and the
            // syntax import brought no implicit conversions into scope.
            if self.st.get(nest_owner).members.iter().any(|&m| {
                self.st.get(m).name == simple
                    && matches!(
                        self.st.get(m).kind,
                        SymKind::Class | SymKind::Module | SymKind::ModuleClass
                    )
            }) {
                continue;
            }
            if !self.load_binary_into(&inner.inner_jvm, nest_owner, span, false) {
                continue;
            }
            if let Some(ev) = scala_module_evidence_type(outer_trait, &simple) {
                mark_nested_module_implicit(&mut self.st, nest_owner, &simple, ev);
            }
        }
    }

    fn ensure_scala_companion(
        &mut self,
        class_id: SymbolId,
        pkg_owner: SymbolId,
        span: Span,
    ) -> SymbolId {
        match self.st.get(class_id).kind {
            SymKind::Module => return class_id,
            SymKind::ModuleClass => {
                let want = self.st.get(class_id).name.trim_end_matches('$').to_string();
                let members = self.st.get(pkg_owner).members.clone();
                return members
                    .into_iter()
                    .find(|&m| {
                        self.st.get(m).kind == SymKind::Module && self.st.get(m).name == want
                    })
                    .unwrap_or(class_id);
            }
            _ => {}
        }
        if let Some(m) = self.st.companion_module(class_id) {
            return m;
        }
        let jvm = self.st.get(class_id).jvm_name.clone();
        if jvm.is_empty() || jvm.ends_with('$') {
            return SymbolId::NONE;
        }
        let comp = format!("{jvm}$");
        self.load_binary_into(&comp, pkg_owner, span, false);
        self.st.companion_module(class_id).unwrap_or(SymbolId::NONE)
    }

    pub(crate) fn complete_java_type(&mut self, ty: &Type, span: Span) {
        match ty {
            Type::Class { sym, args } => {
                self.ensure_java_loaded(*sym, span);
                for a in args {
                    self.complete_java_type(a, span);
                }
            }
            Type::BoundedWildcard { lo, hi } => {
                if let Some(t) = lo {
                    self.complete_java_type(t, span);
                }
                if let Some(t) = hi {
                    self.complete_java_type(t, span);
                }
            }
            Type::Array(t)
            | Type::Repeated(t)
            | Type::ByName(t)
            | Type::Annotated { tpe: t, .. } => {
                self.complete_java_type(t, span);
            }
            _ => {}
        }
    }

    fn complete_java_parents(&mut self, class_id: SymbolId, span: Span) {
        let parents = self.st.get(class_id).parents.clone();
        for p in &parents {
            if let Some(s) = self.st.class_sym_of(p) {
                self.ensure_java_loaded(s, span);
            }
        }
    }

    /// `name` on any parent of a receiver whose upper bound is a **compound**.
    ///
    /// `SymbolTable::class_sym_of` answers with one symbol, and for a
    /// `Type::Refined` it takes the first parent that is a class -- so a
    /// member declared by the *second* half of a bound is unreachable.
    /// `scala.reflect.api.Names` declares
    ///
    /// ```text
    /// type TypeName >: Null <: TypeNameApi with Name
    /// ```
    ///
    /// where `trait TypeNameApi` is empty (it exists only to give `TypeName`
    /// an erased identity) and everything a name can do -- `toTermName`,
    /// `decodedName`, `isTermName` -- comes from `Name`, through `NameApi`.
    /// `symbolOf[R].name.toTermName`, which is how slick's `mapToImpl` gets
    /// at a case class's companion, was "value toTermName is not a member of
    /// Names.TypeName".
    ///
    /// Runs only after the ordinary search and the pickle have both found
    /// nothing, so it can add members and never replace one.
    pub(crate) fn members_through_compound_bound(
        &mut self,
        recv_ty: &Type,
        name: &str,
    ) -> Vec<SymbolId> {
        let id = match recv_ty {
            Type::TypeMember(id) | Type::TypeParam(id) => *id,
            _ => return Vec::new(),
        };
        let Some(Type::Refined { parents, .. }) = self.st.get(id).bound_hi.clone() else {
            return Vec::new();
        };
        let mut out: Vec<SymbolId> = Vec::new();
        for p in &parents {
            if let Some(o) = self.st.class_sym_of(p) {
                for m in self.st.lookup_member(o, name) {
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
            if out.is_empty() {
                for m in self.supply_from_pickle(p, name) {
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
        }
        out
    }

    /// Install `name` on the receiver's class from the library `ScalaSignature`
    /// and return whatever that made visible. Empty unless the receiver is a
    /// standard-library class *and* the member could be expressed faithfully.
    pub(crate) fn supply_from_pickle(&mut self, recv_ty: &Type, name: &str) -> Vec<SymbolId> {
        if !self.library_abi {
            return Vec::new();
        }
        let Some(cls) = self.st.class_sym_of(recv_ty) else {
            // Worth tracing: a receiver that never resolved to a class symbol
            // (a `Type::Named` left behind by a pickle that records member
            // types by simple name) can never be completed, and the user only
            // sees "is not a member".
            crate::pickle_supply::trace(format_args!(
                "#{name}: receiver {} has no class symbol",
                self.st.display_type(recv_ty)
            ));
            return Vec::new();
        };
        crate::pickle_supply::trace(format_args!(
            "#{name}: asking {} ({})",
            self.st.get(cls).name,
            self.st.get(cls).jvm_name
        ));
        // Members found on a companion object land on that module class, not
        // on `cls`, so take what completion reports rather than re-looking-up.
        self.pickle
            .complete(&mut self.st, &mut self.binary, cls, name)
    }

    /// The receiver's *own* declaration of a member it also inherits.
    ///
    /// Library members are read from the pickle on demand and installed on the
    /// class that declares them, so what the typer already has depends on what
    /// earlier code happened to ask for. `Map#collect` is
    /// `MapOps.collect[K2, V2](pf): Map[K2, V2]`, and once some `aMap.collect`
    /// has installed it, `aTreeMap.collect` finds it by inheritance and never
    /// asks `TreeMap` -- whose own `collect(pf)(implicit Ordering[K2]):
    /// TreeMap[K2, V2]` is the one nsc picks. The call then went out as
    /// `IterableOps.collect`, whose default implementation builds through
    /// `iterableFactory`: `TreeMap(1 -> "a").collect(pf)` *returned a `List`*,
    /// with no diagnostic anywhere. Which of the two you got depended on
    /// whether a plain `Map.collect` appeared earlier in the file.
    ///
    /// So when every candidate is inherited, ask the receiver's class too and
    /// union the answers -- `drop_overridden` and the specificity rules then
    /// choose, as they do for a class the typer read in full. Completion is
    /// memoised per `(class, name)`, so this costs one pickle walk per pair.
    pub(crate) fn supply_receiver_override(
        &mut self,
        recv_ty: &Type,
        name: &str,
        found: &mut Vec<SymbolId>,
    ) {
        if !self.library_abi || found.is_empty() {
            return;
        }
        let Some(cls) = self.st.class_sym_of(recv_ty) else {
            return;
        };
        // Nothing to add when the receiver's class already declares one.
        if found.iter().any(|&m| self.st.get(m).owner == cls) {
            return;
        }
        // A real extra JVM signature is an overload, not merely a receiver
        // substitution. Preserve the established overload completion first:
        // TreeMap.map(f)(Ordering) must not disappear when Map.map was loaded
        // by a preceding unit. The result-only path below must not intercept it.
        let have: Vec<Vec<Option<String>>> = found
            .iter()
            .map(|&m| crate::pickle_supply::flat_erased_params(&self.st, &self.st.get(m).ty))
            .collect();
        if self.ancestor_declares_other_signature(cls, name, &have, found) {
            if !self.supply_from_pickle_class(cls, name).is_empty() {
                let now = self.st.lookup_member(cls, name);
                if now.iter().any(|&m| self.st.get(m).owner == cls) {
                    *found = now;
                }
            }
            return;
        }
        let library_collection = self.st.get(cls).jvm_name.starts_with("scala/collection/");
        // And the hierarchy `drop_overridden` is about to order the candidates
        // by is the pickled one for the receiver and for every *library*
        // owner that is not the prelude's. A prelude class is left alone here
        // on purpose, measured: attaching `immutable.Set`'s pickled parents
        // put the prelude's crude `Set.map: ((A) => Any)Set[Any]` below
        // `IterableOps.map[B]` in the same hierarchy, `drop_overridden` then
        // kept only the prelude's, and `aHashSet.map[Int](f)` had no `B` for
        // its type argument (`crates/cli/tests/arraygen.rs`). Duplicated nullary
        // alternatives are resolved in value position by maybe_auto_apply.
        if library_collection {
            let owners: Vec<SymbolId> = found.iter().map(|&m| self.st.get(m).owner).collect();
            for owner in owners.into_iter().chain(std::iter::once(cls)) {
                if !owner.is_none()
                    && owner.0 >= self.st.prelude_end
                    && self.st.get(owner).jvm_name.starts_with("scala/")
                {
                    self.pickle
                        .ensure_parents(&mut self.st, &mut self.binary, owner);
                }
            }
        }
        let all_from_library_binaries = found.iter().all(|&m| {
            let s = self.st.get(m);
            let owner = s.owner;
            !owner.is_none()
                && self.st.get(owner).jvm_name.starts_with("scala/")
                && m.0 >= self.st.prelude_end
                && (!s.pickled_origin.is_empty() || s.jvm_name.starts_with('('))
        });
        // The receiver's copies have to be the *inherited* declarations
        // re-read at the receiver, shape for shape. `PickleSupply::install`
        // keeps one member per erased parameter list, the first the walk
        // offers, so a receiver whose pickle overloads a name on the same
        // erasure -- `HashMap.++` is `MapOps.++[V2 >: V]: CC[K, V2]` over
        // `IterableOps.++[B >: A]: CC[B]` -- can come back with the wrong
        // one (`Iterable[(K, V)]`, and `.get` is not a member of it:
        // `crates/cli/tests/asttype.rs`). The origin says which declaration
        // a copy stands for; a class-file forwarder names none and agrees
        // on its erased parameters instead. A copy of anything else is
        // taken off the class again, because a member left on the receiver
        // is found by every later selection whether or not this one used it.
        if library_collection && all_from_library_binaries {
            let own: Vec<SymbolId> = self
                .pickle
                .complete_on_class(&mut self.st, &mut self.binary, cls, name)
                .into_iter()
                .filter(|&m| self.st.get(m).owner == cls)
                .collect();
            if own.is_empty() {
                return;
            }
            let inherited: Vec<(String, Vec<Option<String>>)> = found
                .iter()
                .map(|&m| {
                    let s = self.st.get(m);
                    (
                        s.pickled_origin.clone(),
                        crate::pickle_supply::flat_erased_params(&self.st, &s.ty),
                    )
                })
                .collect();
            // A copy standing for the receiver's *own* declaration is the
            // override nsc would pick -- `LazyList` redeclares `grouped` and
            // `sliding(size, step)` itself -- and so is one standing for a
            // declaration *below* the inherited candidate's: `SortedMap.map`
            // is `SortedMapOps.map[K2, V2](f)(implicit Ordering[K2])` over
            // `MapOps.map`, and `SortedMapOps` extends `MapOps`. What is
            // refused is a copy of a declaration the inherited one does not
            // reach: `IterableOps.++` next to `MapOps.++` is the sibling the
            // walk should have passed over.
            let own_prefix = format!("{}#", self.st.get(cls).jvm_name.replace('/', "."));
            let declaring = |this: &Self, origin: &str| -> Option<SymbolId> {
                let dotted = origin.split('#').next()?;
                crate::classpath::find_by_jvm(&this.st, &dotted.replace('.', "/"))
            };
            let mut rejected = Vec::new();
            for &m in &own {
                let s = self.st.get(m);
                let params = crate::pickle_supply::flat_erased_params(&self.st, &s.ty);
                let origin = s.pickled_origin.clone();
                let of_shape: Vec<&String> = inherited
                    .iter()
                    .filter(|(_, p)| *p == params)
                    .map(|(o, _)| o)
                    .collect();
                // Judge each alternative independently. An unrelated
                // IterableOps.map copy must not discard SortedMapOps.map's
                // genuine additional Ordering clause alongside it.
                let valid = if of_shape.is_empty() || origin.starts_with(&own_prefix) {
                    true
                } else if let Some(below) = declaring(self, &origin) {
                    if below.0 >= self.st.prelude_end {
                        self.pickle
                            .ensure_parents(&mut self.st, &mut self.binary, below);
                    }
                    of_shape.iter().all(|other| {
                        other.is_empty()
                            || **other == origin
                            || declaring(self, other).is_some_and(|above| {
                                crate::pickle_supply::inherits_from(&self.st, below, above)
                            })
                    })
                } else {
                    false
                };
                if !valid {
                    rejected.push(m);
                }
            }
            self.st
                .get_mut(cls)
                .members
                .retain(|m| !rejected.contains(m));
            if rejected.len() < own.len() {
                *found = self.st.lookup_member(cls, name);
            }
        }
    }

    /// [`Self::declares_other_signature`], asked of the receiver's class *and*
    /// of every ancestor more derived than the answer already in hand.
    ///
    /// A trait declares nothing in its own class file that a `Ops` trait
    /// declares for it: `scala/collection/immutable/SortedMap.class` has no
    /// `map`, no `collect` and no `keySet` -- they are declared by
    /// `collection.SortedMapOps`, which `immutable.SortedMap` inherits. So
    /// asking `cls` alone answers "no" for exactly the family this guard
    /// exists to admit, and `aSortedMap.map(f)` kept whatever a plain
    /// `aMap.map(f)` earlier in the run had installed on `collection.MapOps`.
    ///
    /// The walk stops at the first class that already owns a candidate: past
    /// that point a declaration is not *newer* than the answer in hand, it
    /// *is* the answer in hand or something it overrides, and going further
    /// would re-complete every receiver in the library.
    fn ancestor_declares_other_signature(
        &mut self,
        cls: SymbolId,
        name: &str,
        have: &[Vec<Option<String>>],
        found: &[SymbolId],
    ) -> bool {
        let owners: Vec<SymbolId> = found.iter().map(|&m| self.st.get(m).owner).collect();
        for c in crate::lin::linearize(&self.st, cls) {
            if owners.contains(&c) {
                break;
            }
            if self.declares_other_signature(c, name, have) {
                return true;
            }
        }
        self.pickle_declares_other_arity(cls, name, found)
    }

    /// The receiver's pickle declares `name` somewhere the candidates in hand
    /// do not stand for. The prelude leaves the `…Ops` traits out of its
    /// hierarchy, so the class-file walk above never meets
    /// `SortedSetOps.map[B](f)(implicit ord: Ordering[B]): SortedSet[B]`, and
    /// `aSortedSet.map(f)` resolved to the prelude's plain `immutable.Set.map`:
    /// the call built a `HashSet` (nsc: a `TreeSet`), and a result the typer
    /// had narrowed back to `SortedSet` failed its `checkcast`.
    ///
    /// For each parameter count, the most derived pickled declaration must
    /// be represented. A prelude member stands for any declaration of its
    /// count -- the prelude's own types are what existing programs are
    /// checked against, and `Set.map` does stand for `IterableOps.map` on a
    /// `HashSet`. Once pickled copies are among the candidates, though, they
    /// have to be copies of that very declaration: after some
    /// `aSortedSet.map` has installed `SortedSetOps.map` / `IterableOps.map`
    /// on `SortedSet`, a `BitSet` inherits them, and its own
    /// `BitSetOps.map(f: Int => Int): BitSet` (what nsc picks) would never be
    /// read.
    fn pickle_declares_other_arity(
        &mut self,
        cls: SymbolId,
        name: &str,
        found: &[SymbolId],
    ) -> bool {
        let internal = self.st.get(cls).jvm_name.clone();
        if !internal.starts_with("scala/collection/") {
            return false;
        }
        let decls = self
            .pickle
            .pickled_arities(&mut self.binary, &internal, name);
        let mut firsts: Vec<(usize, String)> = Vec::new();
        for (n, owner) in decls {
            if !firsts.iter().any(|(k, _)| *k == n) {
                firsts.push((n, owner));
            }
        }
        let pickled_in_hand = found
            .iter()
            .any(|&m| !self.st.get(m).pickled_origin.is_empty());
        firsts.iter().any(|(n, owner)| {
            !found.iter().any(|&m| {
                if self.value_param_count(m) != *n {
                    return false;
                }
                let s = self.st.get(m);
                match s.pickled_origin.split_once('#') {
                    Some((origin, _)) => origin == owner,
                    None => !pickled_in_hand || m.0 >= self.st.prelude_end,
                }
            })
        })
    }

    /// Whether `cls`'s own classfile declares an instance method `name` whose
    /// erased parameter list matches none of the candidates in `have`.
    ///
    /// A candidate parameter whose erasure this cannot name is a wildcard, so
    /// an unreadable candidate never *adds* a reason to go to the pickle.
    fn declares_other_signature(
        &mut self,
        cls: SymbolId,
        name: &str,
        have: &[Vec<Option<String>>],
    ) -> bool {
        let internal = self.st.get(cls).jvm_name.clone();
        if internal.is_empty() || !internal.starts_with("scala/") {
            return false;
        }
        let enc = scala_rs_pickle::names::encode_method_name(name);
        let Ok(Some(bytes)) = self.binary.find_class(&internal) else {
            return false;
        };
        let Ok(jc) = crate::javaclass::parse_java_classfile(&bytes) else {
            return false;
        };
        jc.methods.iter().any(|m| {
            if m.name != enc || crate::javaclass::is_java_static(m.access) {
                return false;
            }
            // A bridge is the compiler's own copy of an inherited signature,
            // never a new alternative.
            if crate::javaclass::is_java_bridge(m.access) {
                return false;
            }
            let Some(got) = crate::pickle_supply::desc_params(&m.desc) else {
                return false;
            };
            !have.iter().any(|want| {
                want.len() == got.len()
                    && want
                        .iter()
                        .zip(&got)
                        .all(|(w, g)| w.as_ref().is_none_or(|w| w == g))
            })
        })
    }

    /// The number of value parameters a method takes, across all its clauses --
    /// the arity the JVM sees.
    fn value_param_count(&self, m: SymbolId) -> usize {
        match &self.st.get(m).ty {
            Type::Method { paramss, .. } => paramss.iter().map(|c| c.len()).sum(),
            _ => 0,
        }
    }

    /// [`Self::supply_from_pickle`] for a class symbol that is already known.
    ///
    /// The receiver-typed form reaches this through `class_sym_of`; a wildcard
    /// import (`import <a value>._`) has the class in hand and no receiver type
    /// to hand back.
    /// Read a `-cp` companion object's shape off its `ScalaSignature` pickle
    /// before the `Module[T]` redirect asks it for `apply`.
    ///
    /// A Scala class file on `-cp` gets two readings: the class file itself
    /// (`install_java_class`: erased descriptors plus the JVM generic
    /// signature) and, for the classes `PickleSupply` has *adopted*, the
    /// pickle. Only the pickle records which parameter clause is implicit --
    /// the JVM has no such notion -- and only the pickle can write a higher
    /// kind. `load_binary_into` adopts the class it loads, but a companion
    /// *module* class reached through a package object's re-export
    /// (`val Async = cats.effect.kernel.Async`, which is how `import
    /// cats.effect.Async` arrives) is only ever stubbed by
    /// `find_or_stub_java_class`, which adopts nothing. `Async$` then kept
    /// the class file's `apply(x$0: Async[F]): Async[F]` with an *explicit*
    /// parameter, `complete_named` refused to serve the module class at all,
    /// and `Async[F].flatMap(…)` was "value flatMap is not a member of
    /// `Async$`".
    ///
    /// Only module classes, and only where the redirect is about to ask for
    /// `apply` anyway: adopting a companion installs every member it
    /// declares, which is not something to do speculatively.
    pub(crate) fn adopt_cp_module_class(&mut self, cls: SymbolId) {
        // `adopt_binary_class` declines `java.*` and the prelude's own
        // `scala.*` classes itself; a name that is not a companion's cannot
        // have a companion pickle to read.
        if !self.library_abi
            || cls.is_none()
            || self.st.get(cls).kind != SymKind::ModuleClass
            || !self.st.get(cls).jvm_name.ends_with('$')
        {
            return;
        }
        self.pickle
            .adopt_binary_class(&mut self.st, &mut self.binary, cls);
    }

    /// Repair a `-cp` class that has no constructor at all from its pickle.
    ///
    /// See [`scala_rs_typer::pickle_supply::PickleSupply::supply_ctors`]: a
    /// nested Scala class's own class file carries no `ScalaSignature`, so a
    /// class reached through a type alias (`type Table[T] = …`, which is how
    /// slick exports every one of its abstract classes) is completed from the
    /// enclosing class's pickle -- where constructors are skipped by name.
    pub(crate) fn supply_binary_ctors(&mut self, cls: SymbolId) {
        if !self.library_abi || cls.is_none() {
            return;
        }
        self.pickle
            .supply_ctors(&mut self.st, &mut self.binary, cls);
    }

    pub(crate) fn supply_from_pickle_class(&mut self, cls: SymbolId, name: &str) -> Vec<SymbolId> {
        if !self.library_abi || cls.is_none() {
            return Vec::new();
        }
        self.pickle
            .complete(&mut self.st, &mut self.binary, cls, name)
    }

    /// The module class a *value* reference stands for, if it stands for one.
    ///
    /// A module reference carries `Type::ModuleRef`; the `scala` package
    /// object's aliases (`val Equiv = math.Equiv`) reach us as nullary
    /// accessors, so the same thing also arrives wrapped in a method type with
    /// no parameters. Used by `Module[T]` → `Module.apply[T]`: without it
    /// `Equiv[Int]` kept the module class as its type and `.equiv` was "not a
    /// member of Equiv$".
    pub(crate) fn module_class_of_value(&self, sym: SymbolId, ty: &Type) -> Option<SymbolId> {
        let s = self.st.get(sym);
        if !matches!(s.kind, SymKind::Method | SymKind::Term) || !s.params.is_empty() {
            return None;
        }
        let peeled = match ty {
            Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => {
                (**ret).clone()
            }
            other => other.clone(),
        };
        match peeled {
            Type::ModuleRef(c) => Some(c),
            Type::Class { sym: c, .. } if self.st.get(c).kind == SymKind::ModuleClass => Some(c),
            _ => None,
        }
    }

    pub(crate) fn ensure_java_loaded(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        let jvm = self.st.get(class_id).jvm_name.clone();
        if jvm.is_empty() || jvm.starts_with('[') {
            return;
        }
        // Classpath discovery records only a shallow Scala signature. Adopt
        // the complete one lazily, without re-reading source/prelude classes.
        // Remove first because completion can recursively request this class.
        if self.st.pending_classpath_signatures.remove(&class_id)
            && !self.st.is_source_class(class_id)
            && self
                .pickle
                .adopt_binary_class(&mut self.st, &mut self.binary, class_id)
        {
            // The directory header has only erased parent names. Restore
            // type arguments before inherited members are viewed through
            // this receiver, and complete those parents' declarations too.
            self.pickle
                .ensure_parents(&mut self.st, &mut self.binary, class_id);
            self.complete_java_parents(class_id, span);
        }
        let javaish = self.st.get(class_id).flags.contains(Flags::JAVA)
            || jvm.starts_with("java/")
            || jvm.starts_with("javax/");
        if !javaish {
            return;
        }
        if !self.completed_java.insert(jvm.clone()) {
            return;
        }
        match self.binary.find_class(&jvm) {
            Ok(Some(bytes)) => match crate::javaclass::parse_java_classfile(&bytes) {
                Ok(jc) => {
                    let id = crate::classpath::install_java_class(&mut self.st, &jc);
                    // A `-cp` stub reached through a parent list arrives here
                    // as "javaish" even when it is a Scala trait: see
                    // `adopt_binary_class`.
                    if jc.is_scala {
                        self.pickle
                            .adopt_binary_class(&mut self.st, &mut self.binary, id);
                    }
                    self.complete_java_parents(class_id, span);
                }
                Err(e) => {
                    self.error(
                        span,
                        format!("unsupported classfile {}: {e}", jvm.replace('/', ".")),
                    );
                }
            },
            Ok(None) => {}
            Err(e) => {
                self.error(
                    span,
                    format!("unsupported classfile {}: {e}", jvm.replace('/', ".")),
                );
            }
        }
    }

    /// Complete the concrete aliases needed to reduce an abstract projection
    /// in a binary receiver's generic parent.
    ///
    /// A JVM `Signature` cannot spell `E#TableElementType`: scalac writes
    /// `TableQuery[E] extends Query[E, Object, Seq]` there and keeps the real
    /// parent only in the Scala pickle.  `ensure_java_loaded` restores that
    /// parent as `Query[E, E#TableElementType, Seq]`, but the projection can
    /// only reduce after the concrete argument's own pickle has supplied its
    /// overriding type alias.  That argument is otherwise just a type in the
    /// accessor signature, so ordinary lazy member loading never visits it.
    ///
    /// Do this only for type parameters that actually prefix a retained
    /// projection.  It neither adopts unrelated arguments nor guesses from a
    /// bound: an abstract argument is left for the usual projection rules.
    pub(crate) fn settle_binary_parent_projections(&mut self, recv_ty: &Type, span: Span) {
        if !self.library_abi {
            return;
        }
        let Type::Class { sym, args } = recv_ty else {
            return;
        };
        if sym.is_none() || args.is_empty() {
            return;
        }

        self.pickle
            .ensure_parents(&mut self.st, &mut self.binary, *sym);
        let tparams = self.st.get(*sym).tparams.clone();
        let parents = self.st.get(*sym).parents.clone();
        let mut targets: Vec<(Type, SymbolId)> = Vec::new();
        for parent in &parents {
            for member in self.st.type_members_in(parent) {
                let Some((prefix, decl)) = self.st.abs_projection(member) else {
                    continue;
                };
                let Some(i) = tparams.iter().position(|tp| *tp == prefix) else {
                    continue;
                };
                let Some(arg) = args.get(i) else {
                    continue;
                };
                if matches!(arg, Type::TypeParam(_))
                    || matches!(arg, Type::TypeMember(id) if self.st.is_deferred_type_member(*id))
                    || targets.iter().any(|(t, d)| t == arg && *d == decl)
                {
                    continue;
                }
                targets.push((arg.clone(), decl));
            }
        }

        for (arg, decl) in targets {
            let Some(cls) = self.st.class_sym_of(&arg) else {
                continue;
            };
            self.ensure_java_loaded(cls, span);
            self.pickle
                .ensure_parents(&mut self.st, &mut self.binary, cls);
            let name = self.st.get(decl).name.clone();
            let _ = self
                .pickle
                .complete_type_member(&mut self.st, &mut self.binary, cls, &name);
        }
    }

    /// Read raw members from a Scala classfile even when its pickle has
    /// already been adopted.  Most Scala members are supplied from pickles on
    /// demand; constructor default getters are the one JVM-only exception:
    /// the pickle spells `<init>$default$n`, while the classfile exposes the
    /// static `$lessinit$greater$default$n` forwarder.
    pub(crate) fn ensure_classfile_members_loaded(
        &mut self,
        class_id: SymbolId,
        member_name: &str,
        span: Span,
    ) {
        if class_id.is_none() {
            return;
        }
        // The pickle already supplies the source spelling (`<init>$default$n`)
        // but not the JVM forwarder.  Once that alias is installed, avoid
        // reparsing and reinstalling the whole class for every later default.
        // `install_java_class_in` merges members into the existing symbol and
        // preserves pickle-only flags, but this guard keeps that merge a
        // one-time classfile completion rather than making symbol state depend
        // on the order in which defaults are visited.
        if !member_name.is_empty()
            && self
                .st
                .lookup_member(class_id, member_name)
                .iter()
                .any(|&id| self.st.get(id).kind == SymKind::Method)
        {
            return;
        }
        let jvm = self.st.get(class_id).jvm_name.clone();
        if jvm.is_empty() || jvm.starts_with('[') {
            return;
        }
        let Ok(Some(bytes)) = self.binary.find_class(&jvm) else {
            return;
        };
        let Ok(jc) = crate::javaclass::parse_java_classfile(&bytes) else {
            return;
        };
        let owner = self.st.get(class_id).owner;
        // Reloading JVM members must not erase an already-read Scala parent
        // list: a value class's header says Object, while its pickle says
        // AnyVal. Pickle completion is memoized and would not restore it.
        let scala_parents = (jc.is_scala && self.pickle.parents_loaded(class_id))
            .then(|| self.st.get(class_id).parents.clone());
        let id = crate::classpath::install_java_class_in(&mut self.st, &jc, owner);
        if let Some(parents) = scala_parents {
            self.st.get_mut(id).parents = parents;
        }
        if jc.is_scala {
            self.pickle
                .adopt_binary_class(&mut self.st, &mut self.binary, id);
        }
        self.complete_java_parents(class_id, span);
    }

    /// Mark constructor parameters whose JVM getter exists in a separately
    /// compiled class.  The constructor pickle records the parameter types,
    /// but `supply_ctors` intentionally does not infer `DEFAULTPARAM` from a
    /// classfile: the default body lives on the class's companion and the
    /// classfile has no parameter flag for it.  The forwarder is still a
    /// precise witness (`$lessinit$greater$default$n`), including for
    /// `-Xno-forwarders` and nested classes where only `C$` has the method.
    fn link_existing_nested_companion(&mut self, class_id: SymbolId) {
        if self.st.companion_module(class_id).is_some() {
            return;
        }
        let class_jvm = self.st.get(class_id).jvm_name.clone();
        if class_jvm.is_empty() {
            return;
        }
        let module_jvm = format!("{class_jvm}$");
        let Some(mcls) = crate::classpath::find_by_jvm(&self.st, &module_jvm)
            .filter(|&id| self.st.get(id).kind == SymKind::ModuleClass)
        else {
            return;
        };
        let owner = self.st.get(class_id).owner;
        let module_owner = self.st.get(mcls).owner;
        let outer_module_owner = if !owner.is_none() {
            self.st
                .companion_module(owner)
                .map(|m| self.st.module_class_of(m))
        } else {
            None
        };
        if module_owner != owner && Some(module_owner) != outer_module_owner {
            return;
        }
        let name = self.st.get(class_id).name.clone();
        let Some(module) = self
            .st
            .get(module_owner)
            .members
            .iter()
            .copied()
            .find(|&id| self.st.get(id).kind == SymKind::Module && self.st.get(id).name == name)
        else {
            return;
        };
        if !self.st.get(owner).members.contains(&module) {
            self.st.get_mut(owner).members.push(module);
        }
    }

    pub(crate) fn ensure_external_ctor_defaults(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        self.ensure_classfile_members_loaded(class_id, "", span);
        self.load_companion_module(class_id);
        self.link_existing_nested_companion(class_id);
        if let Some(module) = self.st.companion_module(class_id) {
            let mcls = self.st.module_class_of(module);
            self.ensure_classfile_members_loaded(mcls, "", span);
        }
        // Constructor default ownership is source metadata, not a property of
        // the JVM getter name. Read and merge each pickled constructor against
        // its real descriptor; this also covers an auxiliary constructor whose
        // default getter is forwarded from the class.
        self.supply_binary_ctors(class_id);
    }
}
