#![allow(dead_code)]
//! Statement dispatch and name resolution: how a bare name finds its symbol.
//!
//! Covers `import` -- resolving its prefix, ranking candidate prefixes,
//! package objects, pickled package aliases, named and wildcard selectors --
//! and the lookup that follows: what an unqualified name may see through the
//! enclosing packages, the open wildcards and the classpath, and the binding
//! of the identifier once a candidate is chosen.

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;
use std::collections::{HashMap, HashSet};

impl Typer {
    pub(crate) fn type_stat(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { .. } => {
                // The enclosing block may already have built a `lazy val`'s
                // signature so that earlier statements could name it.
                if tree.id == scala_rs_parser::NodeId(0)
                    || !self.lazy_val_presig.contains(&(self.file_index, tree.id))
                {
                    self.type_val_sig(tree);
                }
                self.type_val_body(tree);
            }
            TreeKind::DefDef { .. } => {
                // `type_member_sig`, not `type_def_sig`: the block above may
                // already have built this signature, and doing it twice would
                // synthesize a second set of evidence parameters.
                self.type_member_sig(tree);
                self.type_def_body(tree);
            }
            TreeKind::Import { .. } => self.type_import(tree),
            // A local `type` alias: the block resolved it before any statement
            // ran (see `TreeKind::Block`), so there is nothing left to do and
            // falling through to `type_expr` would only mark it `Error`.
            TreeKind::TypeDef { .. } => {
                if tree.sym.is_none() {
                    self.namer(tree);
                    self.type_member_sig(tree);
                    self.finish_one_type_alias(tree);
                }
                self.check_stored_annotations(tree);
            }
            // Local `class` / `object` inside a block. `type_expr` routes these
            // back here, so they must not fall through to it again.
            TreeKind::ClassDef { .. } => {
                if tree.sym.is_none() {
                    self.namer_class(tree);
                }
                self.type_local_template(tree);
            }
            TreeKind::ModuleDef { .. } => {
                if tree.sym.is_none() {
                    self.namer_module(tree);
                }
                self.type_local_template(tree);
            }
            _ => {
                self.type_expr(tree, &Type::NoType);
            }
        }
    }

    pub(crate) fn type_import(&mut self, tree: &mut Tree) {
        let expr = match &mut tree.kind {
            TreeKind::Import { expr, .. } => expr,
            _ => return,
        };
        if import_enables_feature(expr, "dynamics") {
            self.language_dynamics = true;
        }
        if import_enables_feature(expr, "postfixOps") {
            self.language_postfix_ops = true;
        }
        if import_enables_feature(expr, "implicitConversions") {
            self.language_implicit_conversions = true;
        }
        let span = tree.span;
        match &mut expr.kind {
            TreeKind::Select { qual, name } if name == "_" => {
                let owners = self.import_prefix(qual, span);
                self.import_wildcard(&owners, &[], span);
            }
            TreeKind::Select { qual, name } if name.starts_with('{') => {
                let sels = decode_import_selectors(name);
                let owners = self.import_prefix(qual, span);
                let hidden: Vec<String> = sels
                    .iter()
                    .filter(|(from, to)| from != "_" && to == "_")
                    .map(|(from, _)| from.clone())
                    .collect();
                let qual = (**qual).clone();
                for (from, to) in &sels {
                    if from == "_" || to == "_" {
                        continue;
                    }
                    self.import_named(&owners, from, to, span, &qual);
                }
                if sels.iter().any(|(from, _)| from == "_") {
                    self.import_wildcard(&owners, &hidden, span);
                }
            }
            TreeKind::Select { qual, name } => {
                let n = name.clone();
                let owners = self.import_prefix(qual, span);
                let qual = (**qual).clone();
                self.import_named(&owners, &n, &n, span, &qual);
            }
            TreeKind::Ident { name } => {
                let n = name.clone();
                for f in self.st.lookup(&n) {
                    self.st.enter_in_current(&n, f);
                }
            }
            _ => {}
        }
        tree.ty = Type::NoType;
    }

    /// The symbol whose members an import's selectors name.
    ///
    /// Packages, objects and package objects are resolved symbolically, one
    /// path segment at a time, so a jar-only package such as `cats.syntax`
    /// never has to be typed as an expression (it has no type). Only a real
    /// term prefix (`import someVal.field._`) falls back to the typer.
    fn import_prefix(&mut self, qual: &mut Tree, span: Span) -> Vec<SymbolId> {
        let syms = self.import_path_syms(qual, span);
        if !syms.is_empty() {
            qual.sym = syms[0];
            return syms.into_iter().map(|s| self.as_type_owner(s)).collect();
        }
        // An import is typed once per pass, and the first pass runs before the
        // enclosing template's `val`s have signatures. `type_select` retypes a
        // qualifier only when it is still `NoType`, so a `Select` that failed
        // in pass one kept its `Error` and never recovered: `import d.p.api._`
        // stayed broken for the whole run while the one-segment-shorter
        // `import d.p._` (whose qualifier is an `Ident`, always retyped)
        // recovered in pass four. That is what `import tdb.profile.api.*`
        // hit in every one of slick's testkit suites.
        //
        // Clearing the path makes the retry a real retry -- but only when the
        // last one did not land, or every pass would re-resolve every import
        // in the run.
        let qspan = qual.span;
        if qual.ty.is_no_type() || qual.ty.is_error() || qual.sym.is_none() {
            clear_path_types(qual);
        }
        self.type_expr(qual, &Type::NoType);
        if !qual.sym.is_none() {
            let id = qual.sym;
            match self.st.get(id).kind {
                SymKind::Module | SymKind::ModuleClass => {
                    self.retract_import_prefix_errors(qspan);
                    return vec![self.st.module_class_of(id)];
                }
                // `import someVal._` / `import c.universe._`: the members are
                // the members of the value's *type*, not of the val symbol,
                // which has none. Falling through to `class_sym_of` below is
                // what makes `import scala.reflect.runtime.universe._` bring
                // in `Tree`, `TermName`, `internal`, ... at all.
                SymKind::Method | SymKind::Term => {}
                _ => {
                    self.retract_import_prefix_errors(qspan);
                    return vec![id];
                }
            }
        }
        match self.st.class_sym_of(&qual.ty) {
            Some(c) => {
                self.retract_import_prefix_errors(qspan);
                let owner = self.st.module_class_of(c);
                if !matches!(
                    self.st.get(owner).kind,
                    SymKind::Module | SymKind::ModuleClass | SymKind::Package
                ) {
                    self.remember_term_import_prefix(owner, qual);
                }
                vec![owner]
            }
            None => {
                self.note_import_prefix_failed(qspan);
                Vec::new()
            }
        }
    }

    /// Drop what an *earlier* pass said about an import prefix that has now
    /// resolved.
    ///
    /// Typing an import prefix is provisional -- the first pass runs before
    /// the enclosing template's `val`s have signatures, so `import p.api._`
    /// with `val p: Profile` in the same template legitimately reports
    /// "value api is not a member of <notype>" on pass one and resolves on
    /// pass four. Diagnostics are deduplicated but never retracted, so that
    /// first attempt was reported for an import that works.
    /// Only prefixes that *did* file something are swept: `diags` grows with
    /// every pass (duplicates are folded at print time, not here), so a
    /// `retain` per resolved import turned a 12-minute measurement into a
    /// 36-minute one on the 240-source testkit run.
    fn retract_import_prefix_errors(&mut self, qspan: Span) {
        if qspan == Span::DUMMY {
            return;
        }
        let key = (self.file_index, qspan.lo.0, qspan.hi.0);
        if !self.import_prefix_failed.remove(&key) {
            return;
        }
        let file = self.file_index;
        self.diags
            .retain(|d| d.file_index != file || d.span.lo < qspan.lo || d.span.hi > qspan.hi);
    }

    /// Remember that this pass could not resolve the prefix, so a later pass
    /// that does knows there is something to retract.
    fn note_import_prefix_failed(&mut self, qspan: Span) {
        self.import_prefix_missed = true;
        if qspan != Span::DUMMY {
            let key = (self.file_index, qspan.lo.0, qspan.hi.0);
            self.import_prefix_failed.insert(key);
        }
    }

    /// Record the prefix `import <a value>._` selects its members through.
    fn remember_term_import_prefix(&mut self, owner: SymbolId, qual: &Tree) {
        if owner.is_none() || qual.ty.is_no_type() || qual.ty.is_error() {
            return;
        }
        // What this import offers, and what the value *is*, both live in the
        // pickled parent list, which nothing has read yet for a class the
        // typer has only named. `import c.universe._` is the case that shows
        // it: `scala.reflect.macros.Universe` extends
        // `scala.reflect.api.Universe`, and until that parent is attached
        // `universe_in_scope` does not recognise the prefix as a universe at
        // all, so every `q"..."` in the body reported "cannot expand".
        if self.library_abi {
            self.pickle
                .ensure_parents(&mut self.st, &mut self.binary, owner);
        }
        // Kept, not replaced. Two imports can name the same class through
        // different values -- a file-level `import scala.reflect.runtime
        // .universe._` and a method-local `import u._` -- and dropping the
        // outer one when the inner is recorded left the *outer* references
        // with no receiver at all once the method ended. Which one applies is
        // decided at each use by `prefix_in_scope`.
        self.term_import_prefixes
            .retain(|(o, q)| !(*o == owner && path_display(q) == path_display(qual)));
        self.term_import_prefixes.push((owner, qual.clone()));
    }

    /// Record the object an *inherited* member was imported through.
    ///
    /// `import scala.util.Random.nextInt` names a member that `object Random`
    /// inherits from `class Random`. The name enters the scope, but the
    /// symbol's owner is the class, so the backend had no receiver to load and
    /// fell back to `this`: `ClassCastException: class Test$ cannot be cast to
    /// class scala.util.Random`. Recording the object as this owner's import
    /// prefix makes [`Self::qualify_term_import`] rewrite the bare name back
    /// into `scala.util.Random.nextInt`, which is what nsc's own `Select`
    /// carries.
    ///
    /// Unlike [`Self::remember_term_import_prefix`] the prefix is a *path*,
    /// resolved symbolically by `import_path_syms` and so typically still
    /// `NoType` here; `qual.sym` is what says it resolved.
    fn remember_named_import_prefix(&mut self, owner: SymbolId, qual: &Tree) {
        if owner.is_none() || qual.sym.is_none() {
            return;
        }
        self.term_import_prefixes
            .retain(|(o, q)| !(*o == owner && path_display(q) == path_display(qual)));
        self.term_import_prefixes.push((owner, qual.clone()));
    }

    /// Whether an `import <a value>._` prefix can still be written here.
    ///
    /// The prefixes are remembered for the whole run, but `import u._` inside
    /// one method says nothing about the next one: `u` is a local there and
    /// gone here. Qualifying with it anyway emitted `getfield` for another
    /// method's local -- a `NoClassDefFoundError` at the first use, from a
    /// program that typechecked.
    ///
    /// The test is the one that matters for the rewrite: does the root of the
    /// path still resolve, here, to the same symbol it did at the import?
    pub(crate) fn prefix_in_scope(&self, qual: &Tree) -> bool {
        let mut t = qual;
        loop {
            match &t.kind {
                TreeKind::Select { qual, .. } => t = qual,
                TreeKind::Ident { .. } if t.sym.is_none() => return true,
                TreeKind::Ident { name } => return self.st.lookup(name).contains(&t.sym),
                // `this.u`, `C.this.u`: reachable wherever the class is.
                _ => return true,
            }
        }
    }

    /// The `import <a value>._` prefix a member of `owner` was reached
    /// through, when that prefix can still be written here.
    ///
    /// Shared by the two places that need it: rewriting a bare name back into
    /// `u.name` for the backend ([`Self::qualify_term_import`]), and reading
    /// an imported implicit at the prefix's type
    /// (`Typer::implicit_candidate_ty`). A member the enclosing class already
    /// has is reached through `this` and is not this import's.
    pub(crate) fn term_import_prefix_for(&self, owner: SymbolId) -> Option<&Tree> {
        if owner.is_none() || self.term_import_prefixes.is_empty() {
            return None;
        }
        if !self.st.this_class.is_none()
            && (owner == self.st.this_class
                || crate::pickle_supply::inherits_from(&self.st, self.st.this_class, owner))
        {
            return None;
        }
        self.term_import_prefixes
            .iter()
            .rev()
            .find(|(o, q)| {
                (*o == owner || crate::pickle_supply::inherits_from(&self.st, *o, owner))
                    && self.prefix_in_scope(q)
            })
            .map(|(_, q)| q)
    }

    /// Turn an unqualified `Literal` that came from `import u._` back into
    /// `u.Literal`, so the backend has a receiver to load.
    ///
    /// Only fires for a member the enclosing class does not itself have: a
    /// name reached through `this` already emits correctly, and rewriting it
    /// would change which symbol it means.
    fn qualify_term_import(&mut self, tree: &mut Tree, name: &str, found: &[SymbolId]) -> bool {
        if self.term_import_prefixes.is_empty() || found.is_empty() {
            return false;
        }
        let owners: Vec<SymbolId> = found
            .iter()
            .map(|&s| self.st.get(s).owner)
            .filter(|o| !o.is_none())
            .collect();
        if owners.is_empty() {
            return false;
        }
        if !self.st.this_class.is_none()
            && owners.iter().any(|&o| {
                o == self.st.this_class
                    || crate::pickle_supply::inherits_from(&self.st, self.st.this_class, o)
            })
        {
            return false;
        }
        let Some(prefix) = owners.iter().find_map(|&o| self.term_import_prefix_for(o)) else {
            return false;
        };
        let qual = prefix.clone();
        let span = tree.span;
        tree.kind = TreeKind::Select {
            qual: Box::new(qual),
            name: name.to_string(),
        };
        tree.ty = Type::NoType;
        tree.sym = SymbolId::NONE;
        tree.span = span;
        true
    }

    /// Resolve `a.b.c` to package / object / class symbols without typing it.
    ///
    /// More than one can answer to the same name: a `case class C` whose
    /// `object C` is named later in the file gets a synthetic companion of its
    /// own, so `import C.member` has to look in both.
    fn import_path_syms(&mut self, t: &Tree, span: Span) -> Vec<SymbolId> {
        match &t.kind {
            TreeKind::Ident { name } if name == "_root_" => vec![self.st.root],
            TreeKind::Ident { name } => {
                self.expose_unqualified(name, span);
                let mut found = self.st.lookup(name);
                if found.is_empty() {
                    found = self.st.lookup_member(self.st.root, name);
                }
                self.rank_import_prefixes(found)
            }
            TreeKind::Select { qual, name } => {
                for owner in self.import_path_syms(qual, span) {
                    let owner = self.as_type_owner(owner);
                    self.complete_binary_member(owner, name, span);
                    let found = self.st.lookup_member(owner, name);
                    let ranked = self.rank_import_prefixes(found);
                    if !ranked.is_empty() {
                        return ranked;
                    }
                    let Some(po) = self.package_object_of(owner, span) else {
                        continue;
                    };
                    self.complete_binary_member(po, name, span);
                    let found = self.st.lookup_member(po, name);
                    let ranked = self.rank_import_prefixes(found);
                    if !ranked.is_empty() {
                        return ranked;
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// An import prefix is a stable identifier, so an object always wins over
    /// a class of the same name (`import scala.util.control.Breaks._` names
    /// the object, not the trait it also inherits from), and a written object
    /// wins over the synthetic companion a `case class` was given.
    fn rank_import_prefixes(&self, found: Vec<SymbolId>) -> Vec<SymbolId> {
        let rank = |k: SymKind| match k {
            SymKind::Package => Some(0),
            SymKind::Module => Some(1),
            SymKind::ModuleClass => Some(2),
            SymKind::Class => Some(3),
            _ => None,
        };
        let mut out: Vec<(u8, u8, SymbolId)> = found
            .into_iter()
            .filter_map(|s| {
                let sym = self.st.get(s);
                let synthetic = u8::from(sym.flags.contains(Flags::SYNTHETIC));
                rank(sym.kind).map(|r| (r, synthetic, s))
            })
            .collect();
        out.sort_by_key(|&(r, syn, s)| (r, syn, s.0));
        out.dedup_by_key(|&mut (_, _, s)| s);
        // Only the best kind answers: a trait and its companion share a name,
        // and `import C._` names the object alone.
        let best = out.first().map(|&(r, _, _)| r);
        out.into_iter()
            .filter(|&(r, _, _)| Some(r) == best)
            .map(|(_, _, s)| s)
            .collect()
    }

    /// `package p { ... }`'s package object, compiled to `p/package$`. Its
    /// members are members of `p` itself. Same-run package objects are folded
    /// into the package by the namer; this covers the ones read from a jar.
    pub(crate) fn package_object_of(&mut self, owner: SymbolId, span: Span) -> Option<SymbolId> {
        if self.st.get(owner).kind != SymKind::Package || owner == self.st.root {
            return None;
        }
        let pkg_jvm = self.st.get(owner).jvm_name.clone();
        if pkg_jvm.is_empty() {
            return None;
        }
        if let Some(id) = self
            .st
            .lookup_member(owner, "package")
            .into_iter()
            .find(|&s| matches!(self.st.get(s).kind, SymKind::Module | SymKind::ModuleClass))
        {
            self.install_duration_syntax(owner, span);
            return Some(self.st.module_class_of(id));
        }
        if !self.load_binary_into(&format!("{pkg_jvm}/package$"), owner, span, true) {
            return None;
        }
        let id = self
            .st
            .lookup_member(owner, "package")
            .into_iter()
            .find(|&s| matches!(self.st.get(s).kind, SymKind::Module | SymKind::ModuleClass))?;
        let mcls = self.st.module_class_of(id);
        // A package object's members are the package's members.
        for mem in self.st.get(mcls).members.clone() {
            if !self.st.get(owner).members.contains(&mem) {
                self.st.get_mut(owner).members.push(mem);
            }
        }
        self.install_pickled_package_aliases(owner, span);
        // `scala.concurrent.duration`'s postfix unit syntax (`5.seconds`)
        // hangs off this package object; see `prelude_durrange.rs`.
        self.install_duration_syntax(owner, span);
        Some(mcls)
    }

    /// A package object's `type` aliases never reach its classfile: scalac
    /// writes them only into the `ScalaSignature` pickle. Folding
    /// `<pkg>/package$`'s *members* into the package therefore leaves
    /// `scala.NoSuchElementException` and `cats.effect.Ref` unresolvable.
    /// Read them from the pickle and enter them as type members of the
    /// package, which is where source code names them from.
    ///
    /// Lazy by construction: this runs when a package object is first needed,
    /// so no package is read ahead of time. An alias whose right-hand side
    /// cannot be rebuilt is *not* installed -- a type member pointing at
    /// nothing would silently mean `Any` -- but it is remembered, so the name
    /// reports why it is missing instead of a bare "not found".
    fn install_pickled_package_aliases(&mut self, pkg: SymbolId, span: Span) {
        if !self.pkg_aliases_done.insert(pkg.0) {
            return;
        }
        let pkg_jvm = self.st.get(pkg).jvm_name.clone();
        if pkg_jvm.is_empty() {
            return;
        }
        let dotted = pkg_jvm.replace('/', ".");
        let Ok(aliases) = self
            .pickle
            .package_object_aliases(&mut self.binary, &format!("{dotted}.package"))
        else {
            // No pickle (a Java-only package, or one compiled in this run):
            // there is nothing to supply, and nothing is claimed.
            return;
        };
        for a in aliases {
            // Anything already there wins: the hand-written prelude, and any
            // real class of the same name. This only fills a hole.
            if a.name.is_empty() || !self.type_owner_members(pkg, &a.name).is_empty() {
                continue;
            }
            match self.pickled_alias_type(&a, span) {
                Some((id, ty)) => {
                    self.st.get_mut(id).ty = ty;
                    self.st.get_mut(id).owner = pkg;
                    self.st.get_mut(pkg).members.push(id);
                }
                None => {
                    self.pkg_alias_gaps
                        .entry(a.name.clone())
                        .or_insert_with(|| {
                            format!(
                                "not found: type {} -- package object {} declares it as an alias \
                             for {}, which this compiler cannot express",
                                a.name,
                                dotted,
                                scala_rs_pickle::sym::render(&a.rhs)
                            )
                        });
                }
            }
        }
    }

    /// Build the symbol for one pickled package-object alias: its own type
    /// parameters first, then its right-hand side.
    ///
    /// The classes the right-hand side names are loaded from the classpath on
    /// demand. The pickle reader can only reach `scala.*` on its own, so
    /// `cats.effect.kernel.Ref` has to be resolved here, where the whole
    /// classpath is available; each round loads what the last one asked for
    /// and tries again, and stops as soon as a round resolves nothing new.
    ///
    /// The symbol is allocated ownerless, so declining leaves the package
    /// untouched.
    fn pickled_alias_type(
        &mut self,
        a: &crate::pickle_supply::PickledAlias,
        span: Span,
    ) -> Option<(SymbolId, Type)> {
        let id = self.st.alloc(
            &a.name,
            SymbolId::NONE,
            SymKind::TypeMember,
            Flags::EMPTY,
            "",
        );
        let mut scope: HashMap<String, Type> = HashMap::new();
        let mut tps = Vec::new();
        for tp in &a.tparams {
            let t = self
                .st
                .alloc(&tp.name, id, SymKind::TypeParam, Flags::EMPTY, "");
            self.st.get_mut(t).ty = Type::TypeParam(t);
            // `type Ref[F[_], A]`: `F` is itself a constructor, and getting its
            // arity right is what keeps `Ref[F, A]` from reporting "does not
            // take type parameters" at the use site.
            let inner: Vec<SymbolId> = (0..tp.arity)
                .map(|i| {
                    let x =
                        self.st
                            .alloc(format!("_${i}"), t, SymKind::TypeParam, Flags::EMPTY, "");
                    self.st.get_mut(x).ty = Type::TypeParam(x);
                    x
                })
                .collect();
            self.st.get_mut(t).tparams = inner;
            scope.insert(tp.name.clone(), Type::TypeParam(t));
            tps.push(t);
        }
        self.st.get_mut(id).tparams = tps;
        // This symbol came from an ALIASsym entry in the provider pickle.
        // Keep that declaration kind even when the RHS resolves to another
        // type member; inferring it from the converted RHS turns aliases such
        // as `type Item[A] = Schema` into abstract TYPEsym entries.
        self.st.get_mut(id).is_type_alias = true;
        let mut asked: HashSet<String> = HashSet::new();
        for _ in 0..8 {
            if let Some(ty) =
                self.pickle
                    .convert_pickled_type(&mut self.st, &mut self.binary, &scope, &a.rhs)
            {
                return Some((id, ty));
            }
            let mut progress = false;
            for m in self.pickle.take_unresolved_refs() {
                if asked.insert(m.clone()) && self.resolve_dotted_class(&m, span).is_some() {
                    progress = true;
                }
            }
            if !progress {
                return None;
            }
        }
        None
    }

    /// The class a pickled dotted name denotes (`cats.effect.kernel.Ref`),
    /// loading it from the classpath if it is not in the table yet.
    ///
    /// Walked one segment at a time rather than turned into a JVM name in one
    /// go, so an object in the middle of the path
    /// (`cats.effect.kernel.Par.ParallelF`) comes out as the nested class it
    /// is, not as a package that does not exist.
    fn resolve_dotted_class(&mut self, dotted: &str, span: Span) -> Option<SymbolId> {
        let mut cur = self.st.root;
        for seg in dotted.split('.') {
            if seg.is_empty() {
                return None;
            }
            let owner = self.as_type_owner(cur);
            // The classfile this segment names, given where the walk is: a
            // member of a package is `p/Seg`, one nested in a class `Outer$Seg`.
            let want = if owner == self.st.root {
                seg.to_string()
            } else {
                let base = self
                    .st
                    .get(owner)
                    .jvm_name
                    .trim_end_matches('$')
                    .to_string();
                match self.st.get(owner).kind {
                    SymKind::Package => format!("{base}/{seg}"),
                    _ => format!("{base}${seg}"),
                }
            };
            self.complete_binary_member(owner, seg, span);
            let mut found = self.type_owner_members(owner, seg);
            // A companion's classfile can already hold the simple name with
            // the module class's JVM name (`Outcome` -> `.../Outcome$`), which
            // carries none of the trait's type parameters. Insist on the
            // classfile the path really names, and read it if the table has
            // not seen it: a type constructor of the wrong arity would make
            // every use of the alias an error.
            if !found.iter().any(|&s| self.st.get(s).jvm_name == want)
                && self.load_binary_into(&want, owner, span, true)
            {
                found = self.type_owner_members(owner, seg);
            }
            cur = found
                .iter()
                .copied()
                .find(|&s| self.st.get(s).jvm_name == want)
                .or_else(|| found.into_iter().next())?;
        }
        Some(cur)
    }

    /// `import p.n` / `import p.{n => alias}`.
    fn import_named(&mut self, owners: &[SymbolId], from: &str, to: &str, span: Span, qual: &Tree) {
        let mut entered = false;
        // Owners of members that came in *inherited* from a superclass of the
        // object named in the import; see `remember_named_import_prefix`.
        let mut inherited_through: Vec<SymbolId> = Vec::new();
        for &owner in owners {
            if owner.is_none() {
                continue;
            }
            self.complete_binary_member(owner, from, span);
            let mut found = self.st.lookup_member(owner, from);
            if found.is_empty() {
                if let Some(po) = self.package_object_of(owner, span) {
                    self.complete_binary_member(po, from, span);
                    found = self.st.lookup_member(po, from);
                    // `complete_binary_member` only ever looks for a *nested
                    // classfile* named after `from` (a companion, an inner
                    // class); an ordinary `val`/`def` member of a package
                    // object is not one -- it is a method entry in `po`'s own
                    // class file, which a plain classfile read already
                    // installed with no way to tell a val's accessor from a
                    // real `def` (see `ident_is_stable`). Only the pickle
                    // knows. `adopt_binary_class` (full member install) is
                    // what nsc's own "adopting the whole companion costs
                    // minutes" comment nearby warns about, so it is not run
                    // unconditionally on every import -- `import scala.
                    // collection.immutable.{ListMap => LM}` from
                    // `imports_jar.scala` alone took over 90s once it was.
                    // `"scala/"`-prefixed classes never need it:
                    // `complete_named` bypasses the adoption gate for them
                    // already (`pickle_supply.rs`). Only a genuinely
                    // non-`"scala/"` package object -- `cats.effect.package`
                    // re-exporting `Resource` is the motivating case -- pays
                    // for full adoption, and only once its targeted, cheap
                    // completion below still finds nothing worth entering.
                    let po_jvm = self.st.get(po).jvm_name.clone();
                    if !po_jvm.starts_with("scala/") {
                        let stale_or_missing = found.is_empty()
                            || found.iter().all(|&s| {
                                self.st.get(s).kind == SymKind::Method
                                    && !self.st.get(s).flags.contains(Flags::ACCESSOR)
                            });
                        if stale_or_missing {
                            let upgraded =
                                self.pickle
                                    .complete(&mut self.st, &mut self.binary, po, from);
                            if !upgraded.is_empty() {
                                found = upgraded;
                            } else {
                                // The targeted lookup itself declines
                                // (`complete_named`'s gate: a non-`"scala/"`
                                // class needs `self.adopted` set before it
                                // will read anything from its pickle at
                                // all) -- pay for full adoption only now,
                                // once memoized per class by `self.adopted`.
                                self.pickle
                                    .adopt_binary_class(&mut self.st, &mut self.binary, po);
                                self.complete_binary_member(po, from, span);
                                found = self.st.lookup_member(po, from);
                            }
                        }
                    }
                }
            }
            // A member of a *value*'s class that only the `ScalaSignature`
            // knows: `import c.{prefix => prefix}` on a
            // `scala.reflect.macros.blackbox.Context`. `import_prefix` already
            // resolved `c` to its class and recorded it as this import's term
            // prefix, but nothing here had ever asked the pickle, so the
            // selector reported `value prefix is not a member of
            // scala.reflect.macros.blackbox.Context` for a member the
            // classfile really declares -- while the *same* member written out
            // as `c.prefix` resolved, because `type_select` does ask
            // (`supply_from_pickle`). The two namespaces disagreeing about the
            // same class is the bug; this is the term half, and the `type`
            // half below already ran unconditionally for the same reason.
            //
            // Targeted and memoised per `(class, name)`, and gated the same
            // way `supply_from_pickle` is: a class whose pickle
            // `complete_named` declines to read is left exactly as it was.
            if found.is_empty() && self.library_abi {
                found = self
                    .pickle
                    .complete(&mut self.st, &mut self.binary, owner, from);
            }
            // One selector can name two different things, and a package's own
            // *class* of that name hid the *term* its package object declares.
            // `import scala.tools.reflect.ToolBox` names both the trait
            // `ToolBox` and, in `scala.tools.reflect.package`, the implicit
            // conversion `def ToolBox(m: ru.Mirror): ToolBoxFactory[ru.type]`
            // -- which is the whole point of that import, since `mirror
            // .mkToolBox()` is a call on what the conversion returns. The
            // trait alone satisfied the loop above, so nothing looked in the
            // package object and `cm.mkToolBox()` was "value mkToolBox is not
            // a member of JavaUniverse.Mirror".
            //
            // Only a `found` with no term in it at all reaches this, so an
            // import that already named a value or an object is untouched.
            if !found.is_empty()
                && !found.iter().any(|&m| {
                    matches!(
                        self.st.get(m).kind,
                        SymKind::Term | SymKind::Method | SymKind::Module | SymKind::ModuleClass
                    )
                })
            {
                if let Some(po) = self.package_object_of(owner, span) {
                    self.complete_binary_member(po, from, span);
                    let mut extra = self.st.lookup_member(po, from);
                    if extra.is_empty() && self.library_abi {
                        extra = self
                            .pickle
                            .complete(&mut self.st, &mut self.binary, po, from);
                    }
                    for m in extra {
                        if !found.contains(&m) {
                            found.push(m);
                        }
                    }
                }
            }
            for m in found {
                self.st.enter_in_current(to, m);
                entered = true;
                let mowner = self.st.get(m).owner;
                if mowner != owner
                    && !mowner.is_none()
                    && self.st.get(owner).kind == SymKind::ModuleClass
                    && !matches!(
                        self.st.get(mowner).name.as_str(),
                        "Any" | "AnyRef" | "AnyVal" | "Object"
                    )
                {
                    inherited_through.push(mowner);
                }
            }
        }
        for mowner in inherited_through {
            self.remember_named_import_prefix(mowner, qual);
        }
        // The two namespaces are separate, and a jar's `type` member leaves no
        // trace in the bytecode: `lookup_member` above can only ever find the
        // *term*. slick's `JdbcBackend` declares both
        // `type Database = DatabaseDef` and `val Database`, and gitbucket
        // writes `import slick.jdbc.JdbcBackend.{Database => SlickDatabase,
        // Session}` -- so `private val db: SlickDatabase` was typed as the
        // *factory object's* class and `Database() withTransaction { … }` was
        // "value withTransaction is not a member of DatabaseFactory", taking
        // every `Session` in the program with it. `Session`, which has no term
        // side at all, was "value Session is not a member of object
        // JdbcBackend". Run whether or not the term was found, for exactly
        // that reason. Same shape as `expose_unqualified_type`.
        if self.library_abi && self.st.lookup_type(to).is_empty() {
            for &owner in owners {
                if owner.is_none() {
                    continue;
                }
                let member =
                    self.pickle
                        .complete_type_member(&mut self.st, &mut self.binary, owner, from);
                match member {
                    Some(Type::TypeMember(id)) => {
                        self.st.enter_in_current(to, id);
                        entered = true;
                        break;
                    }
                    // A nullary alias is its right-hand side and has no symbol
                    // of its own; when that is a plain class, the class is
                    // what the imported name means.
                    Some(Type::Class { sym, args }) if args.is_empty() && !sym.is_none() => {
                        self.st.enter_in_current(to, sym);
                        entered = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        if entered {
            return;
        }
        let Some(&owner) = owners.iter().find(|o| !o.is_none()) else {
            // The prefix itself did not resolve; it reported its own error.
            return;
        };
        // nsc: `import p.Nope` is an error at the selector, not only at the
        // later use of the name.
        self.error(
            span,
            format!("value {from} is not a member of {}", self.owner_desc(owner)),
        );
    }

    /// `package p` / `object O` / `class C`, for a diagnostic.
    pub(crate) fn owner_desc(&self, owner: SymbolId) -> String {
        let s = self.st.get(owner);
        let name = if s.jvm_name.is_empty() {
            s.name.clone()
        } else {
            s.jvm_name.replace('/', ".")
        };
        match s.kind {
            SymKind::Package => format!("package {name}"),
            SymKind::Module | SymKind::ModuleClass => {
                format!("object {}", name.trim_end_matches('$'))
            }
            _ => name,
        }
    }

    /// `import p._` / `import p.*`. Members already known are entered eagerly;
    /// the owner is also recorded so that a name only reachable by reading a
    /// classfile is still found later (see `expose_unqualified`).
    fn import_wildcard(&mut self, owners: &[SymbolId], hidden: &[String], span: Span) {
        // A wildcard whose prefix is a package or an object is enumerable: the
        // walk below enters every member it has. Anything else -- a prefix
        // that did not resolve, or a *value* whose type is a jar class read one
        // name at a time -- leaves names in scope that this compiler never
        // sees, and a name it cannot find in such a file proves nothing.
        if owners.is_empty()
            || owners.iter().any(|&o| {
                o.is_none()
                    || !matches!(
                        self.st.get(o).kind,
                        SymKind::Package | SymKind::Module | SymKind::ModuleClass
                    )
            })
        {
            self.opaque_import_files.insert(self.file_index);
        }
        let mut all: Vec<SymbolId> = Vec::new();
        for &owner in owners {
            if owner.is_none() || all.contains(&owner) {
                continue;
            }
            all.push(owner);
            if let Some(po) = self.package_object_of(owner, span) {
                if !all.contains(&po) {
                    all.push(po);
                }
            }
        }
        for o in all {
            // `import o._` imports what `o` *has*, not only what it declares
            // (SLS 4.7). `cats.syntax.all` is an object whose own member list
            // is empty: every `toFlatMapOps` / `catsSyntaxApplicativeId` comes
            // from one of the ~60 traits it mixes in, so importing the direct
            // members alone brought none of cats' syntax layer into scope.
            // Breadth-first from `o`, so a name a subclass declares is entered
            // ahead of the one it overrides.
            let mut work = std::collections::VecDeque::from([o]);
            let mut walked = std::collections::HashSet::new();
            while let Some(cur) = work.pop_front() {
                if cur.is_none() || !walked.insert(cur.0) {
                    continue;
                }
                // A jar class's members are read one name at a time, and this
                // import asks for no name in particular. Its *implicits* are
                // the ones nothing else will ever ask for -- an implicit is
                // found by searching the scope, not by being written down --
                // so `import seq.integral._` brought neither
                // `Numeric#mkNumericOps` nor `Ordering#mkOrderingOps` into
                // scope and `increment < zero` was
                // `value < is not a member of T`. Only a name the class has
                // no member for is asked, so a hand-written prelude
                // declaration still wins.
                //
                //
                // Asking about a class whose pickle this compiler is not
                // reading yet is worse than useless -- `PickleSupply::complete`
                // declines it *and memoizes the refusal*, so the later
                // `adopt_binary_class` gets that memo instead of the pickled
                // signature. See `docs/gitbucket.md`'s "not fixed" entry on
                // blocking-slick: guarding this loop with
                // `PickleSupply::pickle_readable` is correct and makes
                // gitbucket 50x slower, because implicit search is exponential
                // in the size of the implicit scope.
                if self.library_abi {
                    for n in self
                        .pickle
                        .implicit_member_names(&self.st, &mut self.binary, cur)
                    {
                        if self.st.lookup_member(cur, &n).is_empty() {
                            self.supply_from_pickle_class(cur, &n);
                        }
                    }
                }
                for m in self.st.get(cur).members.clone() {
                    let n = self.st.get(m).name.clone();
                    if n.ends_with('$') || n == "<init>" || hidden.iter().any(|h| h == &n) {
                        continue;
                    }
                    self.st.enter_in_current(&n, m);
                }
                for p in self.st.get(cur).parents.clone() {
                    if let Some(ps) = self.st.class_sym_of(&p) {
                        work.push_back(ps);
                    }
                }
            }
            self.st.enter_wildcard_in_current(o, hidden);
        }
    }

    /// A member the enclosing class inherits from a **`-cp` ancestor**,
    /// written unqualified.
    ///
    /// `enter_inherited_members` snapshots what the ancestors' member lists
    /// hold when the template is entered, and a class read from a jar has
    /// almost nothing there: its members are completed one name at a time,
    /// on demand, and nothing had demanded these. A selection through a
    /// receiver (`t.describe`) reaches them through `supply_from_pickle`; a
    /// bare name inside the subclass body reached nothing at all.
    ///
    /// That is what every slick table body is written as --
    /// `class As(tag: Tag) extends Table[Int](tag, "a") { def id =
    /// column[Int]("id", O.PrimaryKey) }` -- and `column` was
    /// "not found: value column" 514 times in one measurement of the testkit
    /// against slick's published jar.
    ///
    /// Runs only when the name is not in scope at all, and enters exactly
    /// what completion installed, so it can neither shadow nor replace
    /// anything.
    fn expose_inherited_from_binary(&mut self, name: &str) {
        if !self.library_abi || self.st.this_class.is_none() {
            return;
        }
        let cls = self.st.this_class;
        if !self.st.get(cls).is_class_like() {
            return;
        }
        let found = self
            .pickle
            .complete(&mut self.st, &mut self.binary, cls, name);
        for id in found {
            self.st.enter_in_current(name, id);
        }
        if self.st.lookup(name).is_empty() {
            self.expose_from_binary_self_type(name);
        }
    }

    /// The same, for a member offered by a **self type** that is a `-cp`
    /// class rather than a parent.
    ///
    /// SLS 5.1: inside a template whose self type is `S`, `this` conforms to
    /// `S`, so every member of `S` is in scope unqualified. `bind_self_type`
    /// implements that by entering the self type's members into the template
    /// scope -- but, exactly as for a parent read from a jar, a binary class's
    /// member list is empty until something asks for a name, so nothing was
    /// entered.
    ///
    /// gitbucket writes every slick table mix-in that way:
    /// `trait BasicTemplate { self: Table[?] => val userName =
    /// column[String]("USER_NAME") }`, and `column` was
    /// `not found: value column`. A qualified `this.column` resolved, because
    /// `lookup_member` walks the self type and `type_select` completes on
    /// demand; only the bare name did not.
    ///
    /// Only the template the name is written in, not the enclosing ones.
    /// nsc's context chain does reach an outer template's self type, but the
    /// member would then have to be read at the *outer* `this` -- a nested
    /// `trait Inner { def d: Int = dequeue() }` inside
    /// `trait Q { self: PriorityQueue[Int] => … }` must see `dequeue(): Int`,
    /// not the declared `A`, and must call it on `Q.this`. Entering the
    /// symbol alone gives neither, so an outer self type is left alone rather
    /// than answered wrongly. Like the parent case this runs only when the
    /// name resolves to nothing at all, and enters exactly what completion
    /// installed.
    fn expose_from_binary_self_type(&mut self, name: &str) {
        let owner = self.st.this_class;
        let Some(st) = self.st.get(owner).self_type.clone() else {
            return;
        };
        // A compound self type offers the members of every part.
        let roots: Vec<Type> = match &st {
            Type::Refined { parents, .. } => parents.clone(),
            other => vec![other.clone()],
        };
        for root in roots {
            let Some(sc) = self.st.class_sym_of(&root) else {
                continue;
            };
            if sc == owner {
                continue;
            }
            // `complete_named` serves a `-cp` class only once it has been
            // adopted, and nothing adopts a class the program only ever
            // names in a self type. Without this, `complete` skipped the
            // self type's *own* declarations and looked at its ancestors
            // only -- and `column` is declared on `Table` itself.
            self.pickle
                .adopt_binary_class(&mut self.st, &mut self.binary, sc);
            let found = self
                .pickle
                .complete(&mut self.st, &mut self.binary, sc, name);
            for id in found {
                self.st.enter_in_current(name, id);
            }
            if !self.st.lookup(name).is_empty() {
                return;
            }
        }
    }

    /// [`Self::expose_unqualified`] for a name used in *type* position.
    ///
    /// The two namespaces are separate, and the reflection API puts the same
    /// name in both: `import c.universe._` offers a `val TermName` **and** a
    /// `type TermName`. Resolving the value first entered a term under that
    /// name, and `expose_unqualified` then saw the name as already bound and
    /// stopped -- so `val n: TermName = TermName("f")` had a right-hand side
    /// and no left-hand type.
    ///
    /// Only the wildcard-import stage is repeated here. The earlier stages
    /// (the enclosing packages, `scala._`, `java.lang._`) offer both
    /// namespaces at once, so a name they answered is already right --
    /// *provided* `expose_unqualified` actually ran its package-member
    /// search. It bails out as soon as `name` resolves to *anything*
    /// locally, and a sibling module or class of the same name -- forward-
    /// entered by the namer before this file's own definitions are typed --
    /// does exactly that, so the search never runs. That is cats' `Newtype`
    /// encoding: `object NonEmptyLazyList { type Type[+A] <: … }` and,
    /// elsewhere, `type NonEmptyLazyList[+A] = NonEmptyLazyList.Type[A]`
    /// name the same thing in two namespaces, and a bare `NonEmptyLazyList`
    /// used as a type inside `NonEmptyLazyList`'s own file resolved to the
    /// *module* -- kind arity 0 -- because the alias, a member of the same
    /// package folded in from another file's package object, was never
    /// pulled into scope. So the guard here checks
    /// [`SymbolTable::has_real_type_entry`], not `lookup_type().is_empty()`:
    /// the module `lookup_type` offers as a fallback must not look like an
    /// answer already found.
    pub(crate) fn expose_unqualified_type(&mut self, name: &str) {
        if name.is_empty() || self.st.has_real_type_entry(name) {
            return;
        }
        if self.library_abi {
            for owner in self.st.wildcard_owners_for(name) {
                match self
                    .pickle
                    .complete_type_member(&mut self.st, &mut self.binary, owner, name)
                {
                    Some(Type::TypeMember(id)) => {
                        self.st.enter_in_current(name, id);
                        return;
                    }
                    // A *nullary* alias has no symbol of its own -- it is its
                    // right-hand side, and `install_type_alias` deliberately
                    // hands that back rather than an opaque `TypeMember` that
                    // would conform to nothing. When the right-hand side is a
                    // plain class, that class *is* what the imported name means,
                    // so its symbol is what goes into scope. Without this,
                    // `import profile.api.*; def f(t: Tag)` left `Tag`
                    // unresolved: slick's `Aliases` declares `type Tag =
                    // lifted.Tag`, `type Tag`-shaped nullary aliases are how the
                    // whole API surface is exported, and a `Named` parameter type
                    // matched no constructor and no signature
                    // ("type mismatch; found: Tag required: Tag").
                    Some(Type::Class { sym, args }) if args.is_empty() && !sym.is_none() => {
                        self.st.enter_in_current(name, sym);
                        return;
                    }
                    _ => {}
                }
            }
        }
        // Not from a jar: a source package member. `lookup_member` already
        // sees a package object's members through the package (see
        // `package_object_of`'s "a package object's members are the
        // package's members"), so no completion is needed here -- only the
        // scope injection `expose_unqualified` would have done, had it not
        // stopped short.
        let from = if !self.st.this_class.is_none() {
            self.st.this_class
        } else {
            self.st.owner
        };
        for pkg in self.open_packages(from) {
            for id in self.st.lookup_member(pkg, name) {
                if matches!(
                    self.st.get(id).kind,
                    SymKind::TypeMember | SymKind::TypeParam | SymKind::Class
                ) {
                    self.st.enter_in_current(name, id);
                }
            }
            if self.st.has_real_type_entry(name) {
                break;
            }
        }
    }

    /// Is `outer` `inner` itself, or one of its enclosing packages?
    fn encloses_package(&self, outer: SymbolId, inner: SymbolId) -> bool {
        let mut cur = inner;
        for _ in 0..64 {
            if cur == outer {
                return true;
            }
            let owner = self.st.get(cur).owner;
            if owner.is_none() || owner == cur {
                return false;
            }
            cur = owner;
        }
        false
    }

    /// The package scopes an unqualified name may be looked up in, innermost
    /// first, ending at the root.
    ///
    /// The file's `package` clauses decide this, not the owner chain: only a
    /// clause *opens* a package. A file that reaches here without a recorded
    /// clause (a lazy completion driven from another unit) gets the strict
    /// answer -- its own package and the root.
    pub(crate) fn open_packages(&self, from: SymbolId) -> Vec<SymbolId> {
        let encl = self.enclosing_package(from);
        let mut out = vec![encl];
        if let Some(opened) = self.open_pkgs.get(&self.file_index) {
            // Innermost first, and only the ones this definition is actually
            // inside: two sibling clauses in one file do not see each other.
            for &p in opened.iter().rev() {
                if p != encl && !out.contains(&p) && self.encloses_package(p, encl) {
                    out.push(p);
                }
            }
        }
        if !out.contains(&self.st.root) {
            out.push(self.st.root);
        }
        out
    }

    pub(crate) fn expose_unqualified(&mut self, name: &str, span: Span) {
        if name.is_empty() || !self.st.lookup(name).is_empty() {
            return;
        }
        let from = if !self.st.this_class.is_none() {
            self.st.this_class
        } else {
            self.st.owner
        };
        self.expose_inherited_from_binary(name);
        if !self.st.lookup(name).is_empty() {
            return;
        }
        // Only the packages the file's own clauses opened, innermost first.
        //
        // A *qualified* clause `package p.q` sees neither a class nor a
        // subpackage of `p` (2.13.16: "not found: type Widget" /
        // "not found: value cats", with and without `-Xsource:3`), while the
        // nested spelling `package p { package q { … } }` sees both. Walking
        // the owner chain instead made slick's own `slick.cats` package
        // shadow the real `cats` for every file under `package slick.*`:
        // `cats.effect.IO` in `package slick.dbio` came out as
        // "value effect is not a member of <notype>".
        //
        // The root package stays at the end of the walk: nsc's context chain
        // for `package p.q` is `q` then the root, and a *qualified*
        // reference like slick's `slick.ControlsConfig` from
        // `package slick.jdbc` resolves its head there. Dropping it from the
        // walk (rather than only the packages in between) is what cost
        // `agent/cats2` a net +1 error.
        for pkg in self.open_packages(from) {
            self.complete_binary_member(pkg, name, span);
            for id in self.st.lookup_member(pkg, name) {
                self.st.enter_in_current(name, id);
            }
            if !self.st.lookup(name).is_empty() {
                break;
            }
        }
        let pkg = self.enclosing_package(from);
        // An `import p._` the program wrote outranks the implicit
        // `import scala._` and `import java.lang._` every source carries: SLS
        // 2 makes those two wildcard imports at the outermost nesting level,
        // so any import written in the file shadows them. While this ran
        // *after* them, `import c.universe._; Function(vparams, body)`
        // resolved to `scala.Function` -- an object with no `apply` -- and the
        // macro implementation slick writes could not be compiled at all.
        //
        // The eager half of `import_wildcard` is not affected: a name it could
        // enter is already in the current scope and neither branch runs.
        self.expose_from_wildcards(name, span);
        if self.st.lookup(name).is_empty() {
            // Every Scala source has an implicit `import scala._`, which ranks
            // above `java.lang._`. Almost every name it offers is already in
            // the prelude, so what this reaches in practice is the `scala`
            // package object's pickled type aliases -- `NoSuchElementException`,
            // `Seq`, `Iterable` -- which `complete_binary_member` installs on
            // the package the first time one is asked for.
            if let Some(sp) = self.scala_package() {
                self.complete_binary_member(sp, name, span);
                for id in self.st.lookup_member(sp, name) {
                    self.st.enter_in_current(name, id);
                }
            }
        }
        if self.st.lookup(name).is_empty() {
            // Every Scala source has an implicit `import java.lang._`.
            if let Some(jl) = self.java_lang_package() {
                self.complete_binary_member(jl, name, span);
                for id in self.st.lookup_member(jl, name) {
                    self.st.enter_in_current(name, id);
                }
            }
        }
        if self.st.lookup(name).is_empty() && pkg != self.st.root {
            self.complete_binary_member(self.st.root, name, span);
            for id in self.st.lookup_member(self.st.root, name) {
                self.st.enter_in_current(name, id);
            }
        }
    }

    /// The lazily-read half of a wildcard import.
    ///
    /// `import p._` where `p` is a jar package, or a value whose class comes
    /// from a jar: those members are read one name at a time, so a name
    /// nothing has asked for yet is not on the owner's member list and
    /// `import_wildcard` could not enter it eagerly.
    fn expose_from_wildcards(&mut self, name: &str, span: Span) {
        if !self.st.lookup(name).is_empty() {
            return;
        }
        for owner in self.st.wildcard_owners_for(name) {
            self.complete_binary_member(owner, name, span);
            let mut found = self.st.lookup_member(owner, name);
            if found.is_empty() {
                // `import <a value>._` where the value's class comes from a
                // jar. The members of such a class are read from its pickle
                // *on demand*, one name at a time, and the ones this import
                // offers are mostly inherited: `import
                // scala.reflect.runtime.universe._` names a `JavaUniverse`,
                // but `TermName` / `Literal` / `Constant` are declared on
                // `scala.reflect.api.Names` / `Trees` / `Constants` far up
                // its linearisation, so nothing had read them yet and the
                // import brought in nothing at all. Selecting the same
                // member through the path (`u.TermName`) always worked --
                // that route runs the completion below -- which is why
                // reified quasiquotes, which build `u.TermName(...)`
                // explicitly, did not notice.
                found = self.supply_from_pickle_class(owner, name);
            }
            if found.is_empty() && self.library_abi {
                // The type namespace, which the reflection API is written
                // in: `import c.universe._` is what puts `Tree`, `Symbol`
                // and `TermName` in scope as *types*, and they are
                // abstract type members of `scala.reflect.api.Trees` /
                // `Symbols` / `Names`. Completing one installs a symbol on
                // its declaring trait, which `lookup_member` then reaches.
                match self
                    .pickle
                    .complete_type_member(&mut self.st, &mut self.binary, owner, name)
                {
                    Some(Type::TypeMember(id)) => found = vec![id],
                    // A nullary alias is its right-hand side and has no
                    // symbol; the class it names is what the import offers
                    // under that name. See `expose_unqualified_type`.
                    Some(Type::Class { sym, args }) if args.is_empty() && !sym.is_none() => {
                        found = vec![sym]
                    }
                    _ => {}
                }
            }
            if found.is_empty() {
                // `complete_binary_member` only ever looks for a *nested
                // classfile* named after `name` (a companion, an inner
                // class) -- exactly what `import integral._; zero` /
                // `fromInt(5)` are not: `zero` and `fromInt` are ordinary
                // methods `scala.math.Numeric`'s own pickle declares,
                // with no classfile of their own. `import_wildcard`
                // (the eager half of a wildcard import) already snapshots
                // whatever is on the owner's member list *at import
                // time*; a standard-library trait's members are mostly
                // read from its pickle on demand, so a name nothing had
                // asked for yet was never in that snapshot. The pickle
                // path a plain member selection already uses
                // (`supply_from_pickle`) is the one this needs too.
                found = self
                    .pickle
                    .complete(&mut self.st, &mut self.binary, owner, name);
            }
            if found.is_empty() {
                continue;
            }
            for id in found {
                self.st.enter_in_current(name, id);
            }
            break;
        }
    }

    pub(crate) fn type_ident(&mut self, tree: &mut Tree, name: String, pt: &Type) {
        if name == "_" {
            self.error(tree.span, "unbound placeholder parameter");
            tree.kind = TreeKind::Wildcard;
            tree.ty = Type::Error;
            return;
        }
        self.expose_unqualified(&name, tree.span);
        // A `TupleN` the parser made up for `(a, b)` is nsc's fully qualified
        // `scala.TupleN`, so it is resolved in package `scala` and cannot be
        // captured -- `object Ordering` declares `implicit def Tuple2[T1, T2]`
        // and writes tuple literals in its own body.
        if tree.scala_ref {
            let found = self.st.lookup_scala(&name);
            if !found.is_empty() {
                // Term position: the companion `object TupleN` carries the
                // `apply`, so it wins over the class of the same name, exactly
                // as in the general path below.
                let modules: Vec<SymbolId> = found
                    .iter()
                    .copied()
                    .filter(|s| {
                        matches!(
                            self.st.get(*s).kind,
                            SymKind::Module | SymKind::Method | SymKind::Term
                        )
                    })
                    .collect();
                let found = if modules.is_empty() { found } else { modules };
                self.bind_found(tree, found, pt);
                return;
            }
        }
        let mut found = self.st.lookup(&name);
        // See `SymbolTable::lookup_extractor`: in a constructor pattern a
        // `def` of the name is not an extractor and does not shadow one.
        if self.ctor_pattern_fun
            && !found.is_empty()
            && found
                .iter()
                .all(|&s| self.st.get(s).kind == SymKind::Method)
        {
            let alt = self.st.lookup_extractor(&name);
            if !alt.is_empty() {
                found = alt;
            }
        }
        // A scope that binds the name only in the *type* namespace does not
        // hide a term of that name further out: `import syntax._` bringing a
        // `type HNil` alias into scope leaves `object HNil` reachable.
        if !found.iter().any(|s| {
            matches!(
                self.st.get(*s).kind,
                SymKind::Module | SymKind::ModuleClass | SymKind::Method | SymKind::Term
            )
        }) {
            let terms = self.st.lookup_term(&name);
            if !terms.is_empty() {
                found = terms;
            }
        }
        if found.is_empty() {
            found = self.st.lookup_member(self.st.root, &name);
        }
        // `_root_` names the root package. nsc binds it in every scope;
        // scala-rs understood it only in an import path, so
        // `_root_.scala.List(1, 2)` -- what a macro writes to keep its
        // expansion out of whatever the call site happens to have in scope,
        // and what slick's `mapToImpl` writes eleven times -- was "not found:
        // value _root_". Looked up first, so a binding of that name (which
        // Scala does not allow, but a class file could still carry) wins.
        if found.is_empty() && name == "_root_" {
            found = vec![self.st.root];
        }
        if found.is_empty() {
            // A tree that already resolved keeps its symbol. The typer types
            // some applications twice -- `retry_tupled_args` re-runs a call
            // whose implicit arguments the first pass already filled in -- and
            // a synthesized implicit reference (`ScalaBaseType.intType`,
            // `<:<.refl`) names a companion member that was never in lexical
            // scope. Re-resolving it by name would report `not found: value
            // intType` for a reference the search had already settled.
            if !tree.sym.is_none()
                && self.st.get(tree.sym).name == name
                && !tree.ty.is_error()
                && !tree.ty.is_no_type()
            {
                return;
            }
            self.not_found_error(tree.span, "value", &name);
            tree.ty = Type::Error;
            return;
        }
        // Term position prefers modules/methods/vals over the class of the same name.
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
        let found = if terms.is_empty() {
            // Nothing in the term namespace under this name, and a class of it
            // is in scope: its companion may simply not have been read yet.
            // See [`Self::expose_class_companion`].
            self.expose_class_companion(&found, &name, tree.span)
        } else {
            terms
        };
        if self.qualify_term_import(tree, &name, &found) {
            self.type_select(tree, pt);
            return;
        }
        self.bind_found(tree, found, pt);
    }

    /// The companion object of a class that is in scope under this name, read
    /// from its class file if nothing has read it yet.
    ///
    /// A class file's companion is a second class file (`p/C.class` and
    /// `p/C$.class`), and it is installed only when something asks for that
    /// name. `import p._` over a *package* asks for no name in particular:
    /// `import_wildcard` walks the package's member list and enters what is
    /// already there, which for a `-cp` package is the classes alone. Once the
    /// class `C` is in the current scope, `expose_unqualified` returns at its
    /// first line -- the name resolves -- so the companion is never read, and
    /// `C[Arg]` in *term* position bound the class. The `Module[T]` →
    /// `Module.apply[T]` redirect needs a module symbol and got a class, so
    /// slick's `TableQuery[Issues]` came back as the class's own type with `E`
    /// unsubstituted (`value label is not a member of E`). Writing the
    /// companion's members behind another object -- slick's `api` re-exports
    /// `val TableQuery = lifted.TableQuery`, which is a *term* and so is
    /// entered by the same walk -- is what hid this.
    ///
    /// Returns the modules when there are any, so term position binds them
    /// (SLS 2: the two namespaces are separate, and this is term position);
    /// otherwise the classes it was given, unchanged.
    pub(crate) fn expose_class_companion(
        &mut self,
        found: &[SymbolId],
        name: &str,
        span: Span,
    ) -> Vec<SymbolId> {
        let classes: Vec<SymbolId> = found
            .iter()
            .copied()
            .filter(|&s| self.st.get(s).kind == SymKind::Class)
            .collect();
        if classes.is_empty() {
            return found.to_vec();
        }
        let mut modules = Vec::new();
        for cls in classes {
            if self.st.companion_module(cls).is_none() {
                let jvm = self.st.get(cls).jvm_name.clone();
                if jvm.is_empty() || jvm.starts_with('[') {
                    continue;
                }
                let owner = self.st.get(cls).owner;
                self.load_binary_into(&format!("{jvm}$"), owner, span, true);
            }
            if let Some(m) = self.st.companion_module(cls) {
                if !modules.contains(&m) {
                    modules.push(m);
                }
            }
        }
        if modules.is_empty() {
            return found.to_vec();
        }
        for &m in &modules {
            self.st.enter_in_current(name, m);
        }
        modules
    }

    /// An inherited member's type, read through the class the unqualified
    /// reference is written in.
    ///
    /// The single-alternative path in [`Self::bind_found`] does this inline,
    /// with the same three exclusions (a self alias names the *enclosing*
    /// instance, a local or parameter has no prefix at all, and a
    /// `private[this]` member is not inherited, SLS 5.2). An *overloaded*
    /// name needed it just as much and did not have it: every alternative
    /// went into `Type::Overload` in its declaring class's vocabulary.
    ///
    /// Twirl's `BaseScalaTemplate[T <: Appendable[T], F <: Format[T]]`
    /// declares six `_display_` overloads, all returning `T`, and every
    /// generated template calls `_display_ { … }` unqualified from inside
    /// `object x extends BaseScalaTemplate[Html, Format[Html]]`. The result
    /// came back as the bare `T`, so the template's own declared result type
    /// did not match it.
    fn ident_ty_as_seen_from_this(&self, s: SymbolId, ty: Type) -> Type {
        if self.st.this_class.is_none() {
            return ty;
        }
        let owner = self.st.get(s).owner;
        if owner == self.st.this_class || owner.is_none() {
            return ty;
        }
        if self.st.get(owner).self_alias == Some(s) {
            return ty;
        }
        if !matches!(
            self.st.get(owner).kind,
            SymKind::Class | SymKind::ModuleClass | SymKind::Module
        ) {
            return ty;
        }
        let f = self.st.get(s).flags;
        if f.contains(Flags::PRIVATE) && f.contains(Flags::LOCAL) {
            return ty;
        }
        let this_ty = Type::Class {
            sym: self.st.this_class,
            args: self
                .st
                .get(self.st.this_class)
                .tparams
                .iter()
                .map(|t| Type::TypeParam(*t))
                .collect(),
        };
        self.st.subst_as_seen_from(&this_ty, &ty)
    }

    fn bind_found(&mut self, tree: &mut Tree, mut found: Vec<SymbolId>, pt: &Type) {
        found.sort_by_key(|s| s.0);
        found.dedup();
        // The template scope holds a class's own members next to the inherited
        // ones, so `val symbolName` and the `def symbolName` it implements both
        // answer to the name. An override is one member, not an overload.
        found = self.drop_overridden(found);
        let ref_span = tree.span;
        for s in found.iter().copied() {
            self.complete_lazy_sig(s, ref_span);
        }
        if found.len() == 1 {
            let s = found[0];
            tree.sym = s;
            let mut ty = self.st.get(s).ty.clone();
            if ty.is_no_type()
                && self.st.get(s).flags.contains(Flags::PARAM)
                && is_inferable_param_pt(pt)
            {
                self.st.get_mut(s).ty = pt.clone();
                ty = pt.clone();
            }
            // An inherited member is seen through this class: `find` declared
            // on `Repo[A]` is `(User => Boolean) => Option[User]` inside
            // `class UserStore extends Repo[User]`.
            if !self.st.this_class.is_none() {
                let owner = self.st.get(s).owner;
                // A self alias names the *enclosing* instance, not this one:
                // inside `new CI[B] { def close() = self.close() }` the `self`
                // of the surrounding `trait CI { self => }` still stands for
                // the trait's own `this`, so its `CI.this.type` must not be
                // re-read as the anonymous class.
                let is_self_alias = self.st.get(owner).self_alias == Some(s);
                // Only a *class's* member is seen through a prefix. A local or
                // a parameter of an enclosing method is owned by that method,
                // and its type is written in the method's own vocabulary: in
                // `trait It[T] { def map[B](f: T => B) = new It[B] { … f … } }`
                // the `T` of `f` is the enclosing trait's, not the one the
                // anonymous class's parent `It[B]` binds.
                let owner_is_class = matches!(
                    self.st.get(owner).kind,
                    SymKind::Class | SymKind::ModuleClass | SymKind::Module
                );
                // A `private[this]` member is not inherited (SLS 5.2), so the
                // only prefix an unqualified reference to one can have is its
                // *own* class's `this` -- never the class we happen to be
                // inside. slick's `SynchronousDatabaseAction` writes
                //
                //   private[this] def superZip[R2, E2](a: …) = super.zip(a)
                //   override def zip[R2, E2](a: …) = … new Fused[(R, R2), …] {
                //     override def nonFusedEquivalentAction = superZip(a)
                //   }
                //
                // and the anonymous class is another `SynchronousDatabaseAction`,
                // at `R = (R, R2)`. Reading `superZip` through it turned
                // `DBIOAction[(R, R2), …]` into `DBIOAction[((R, R2), R2), …]`
                // (and `superAsTry` into `Try[Try[R]]`). `enter_inherited_members`
                // already keeps such a member out of the subclass scope, so the
                // name resolved to the enclosing class's own -- only the
                // prefix was wrong. With a *public* `superZip` nsc reports the
                // very mismatch we did, so this is exactly the `private[this]`
                // case and nothing wider.
                let f = self.st.get(s).flags;
                let private_this = f.contains(Flags::PRIVATE) && f.contains(Flags::LOCAL);
                // Reaching it from a *different* class means the JVM sees a
                // cross-class call to an `ACC_PRIVATE` member, which is an
                // `IllegalAccessError` however well it type-checks. Same
                // widening `note_companion_access` performs for a companion
                // read (which deliberately skips `LOCAL`, because until now
                // nothing else could reach one).
                if private_this && owner != self.st.this_class && !owner.is_none() && owner_is_class
                {
                    self.st.get_mut(s).access_widened = true;
                }
                if owner != self.st.this_class
                    && !owner.is_none()
                    && !is_self_alias
                    && !private_this
                    && owner_is_class
                {
                    let this_ty = Type::Class {
                        sym: self.st.this_class,
                        args: self
                            .st
                            .get(self.st.this_class)
                            .tparams
                            .iter()
                            .map(|t| Type::TypeParam(*t))
                            .collect(),
                    };
                    ty = self.st.subst_as_seen_from(&this_ty, &ty);
                }
            }
            ty = self.maybe_auto_apply(ty, pt);
            ty = self.instantiate_parameterless(s, ty, pt);
            // Only a *member* of a class is seen through `this`. A local or a
            // parameter has no prefix, and its type is already written in the
            // vocabulary of the method that owns it: `val n = map.infer(x)`
            // inside a class that declares `type Self = RSM` has the type
            // `map.Self`, which is the abstract member of `map`'s own class --
            // rebinding it to this class's `Self` gave the tuple destructuring
            // `checkcast RSM` on a value that is only a `Node`
            // (`ClassCastException` in slick's `ResultSetMapping
            // .withInferredType`, reached by every query the compiler runs).
            if !self.st.this_class.is_none()
                && matches!(
                    self.st.get(self.st.get(s).owner).kind,
                    SymKind::Class | SymKind::ModuleClass | SymKind::Module
                )
            {
                ty = self.st.expand_type_members(self.st.this_class, &ty);
            }
            tree.ty = ty;
            return;
        }
        // The same member can be reached twice (inherited through two parents,
        // or entered by both a package and its package object). Alternatives
        // that agree on their type are one member, not an overload.
        let first_ty = self.st.get(found[0]).ty.clone();
        if !first_ty.is_no_type() && found.iter().all(|&s| self.st.get(s).ty == first_ty) {
            found.truncate(1);
            let s = found[0];
            tree.sym = s;
            let ty = self.ident_ty_as_seen_from_this(s, first_ty);
            let ty = self.maybe_auto_apply(ty, pt);
            tree.ty = self.instantiate_parameterless(s, ty, pt);
            return;
        }
        // Keep overloads intact so `println(1)` can still pick a 1-arg alternative.
        // Nullary alternatives still auto-apply in value position (`"x".stripMargin`).
        let ov_name = self.st.get(found[0]).name.clone();
        self.record_overload_group(&found, &ov_name);
        // As seen from this class, exactly as the single-alternative branch
        // above and as `type_select` does for a receiver. The types have to be
        // filed under `overload_member_types` as well, because
        // `resolve_overload_with` rebuilds its candidates from the symbols and
        // would otherwise read every alternative raw again.
        let alts: Vec<(SymbolId, Type)> = found
            .iter()
            .map(|&s| {
                let t = self.st.get(s).ty.clone();
                (s, self.ident_ty_as_seen_from_this(s, t))
            })
            .collect();
        if alts.iter().any(|(s, t)| &self.st.get(*s).ty != t) {
            self.overload_member_types.insert(found[0].0, alts.clone());
        }
        let ov = Type::Overload(alts.iter().map(|(_, t)| t.clone()).collect());
        tree.ty = self.maybe_auto_apply(ov, pt);
        // The same rule the receiver form goes through in `type_select`: one
        // alternative whose parameters are all implicit is what value position
        // keeps, and `maybe_auto_apply` cannot recognise it from the type
        // alone. gitbucket's controllers write `params.get(…)` unqualified,
        // through `ScalatraFilter`'s own inherited member, so this is the half
        // of the pair that carries the count. See
        // `implicit_only_alternative`.
        if matches!(tree.ty, Type::Overload(_))
            && !matches!(pt, Type::Function { .. } | Type::Method { .. })
        {
            if let Some(id) = self.implicit_only_alternative(&alts) {
                if let Some((_, t)) = alts.iter().find(|(s, _)| *s == id) {
                    tree.ty = t.clone();
                }
                tree.sym = id;
                if id != found[0] {
                    self.overload_member_types.insert(id.0, alts.clone());
                    if let Some(g) = self.overload_groups.get(&found[0].0).cloned() {
                        self.overload_groups.insert(id.0, g);
                    }
                }
                return;
            }
        }
        tree.sym = if matches!(tree.ty, Type::Overload(_)) {
            found[0]
        } else {
            found
                .iter()
                .copied()
                .find(|&s| self.is_nullary_method_sym(s))
                .or_else(|| {
                    found
                        .iter()
                        .copied()
                        .find(|&s| self.is_parameterless_sym(s))
                })
                .unwrap_or(found[0])
        };
    }
}
