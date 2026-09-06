#![allow(dead_code)]
//! Supplying the arguments a call did not write out.
//!
//! Named arguments (matching them to parameters and putting them back in
//! order), parameter defaults and the getters that hold them, and implicit
//! parameter lists -- searched for, or synthesised. The tail of the file is
//! evidence the compiler materialises rather than finds: `ClassTag`,
//! `TypeTag`/`WeakTypeTag` and the two hard-wired views (`identity` and
//! wrapping an `Array` into an `ArrayOps`).

use crate::check::*;
use crate::implicits::ImplicitSearch;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    /// `Left.apply` / `Right.apply` → `Left[A, B]` / `Right[A, B]`.
    /// The value argument fills `A` (Left) or `B` (Right); the other param
    /// comes from an expected `Either[A, B]` (or `Nothing` if none).
    ///
    /// `ret` is the `apply`'s own declared result, and the class is taken from
    /// it rather than looked up by simple name. The shortcut used to key off
    /// the owner module's name alone and then ask the scope for a class called
    /// `Left`, which is only right for `scala.util.Left`: cats' `Ior` declares
    ///
    /// ```scala
    /// final case class Left[+A](a: A) extends Ior[A, Nothing]
    /// final case class Right[+B](b: B) extends (Nothing Ior B)
    /// ```
    ///
    /// so every `Ior.Left(a)` -- even written out as `cats.data.Ior.Left(a)` --
    /// was typed as a `scala.util.Left[A, B]` and reported `type mismatch;
    /// found: Left[String, Int]  required: Ior[String, Int]`. A one-parameter
    /// `Left` is not the class this rule is about, and the arity check says so
    /// on top of the identity check, since `value_idx` is `Either`'s layout.
    pub(crate) fn instantiate_either_ctor_apply(
        &self,
        owner_n: &str,
        ret: &Type,
        args: &[Tree],
        pt: &Type,
    ) -> Option<Type> {
        let (cname, value_idx) = match owner_n {
            "Left$" => ("Left", 0usize),
            "Right$" => ("Right", 1usize),
            _ => return None,
        };
        let Type::Class { sym: cls, .. } = ret else {
            return None;
        };
        let cls = *cls;
        if self.st.get(cls).name != cname || self.st.get(cls).tparams.len() != 2 {
            return None;
        }
        let val_ty = args.first()?.ty.widen_constant();
        let n = self.st.get(cls).tparams.len();
        let pt_args: &[Type] = match pt {
            Type::Class { args, .. } if args.len() == n => args,
            _ => &[],
        };
        let mut inferred = Vec::with_capacity(n);
        for i in 0..n {
            if i == value_idx {
                inferred.push(val_ty.clone());
            } else {
                inferred.push(pt_args.get(i).cloned().unwrap_or(Type::Nothing));
            }
        }
        Some(Type::Class {
            sym: cls,
            args: inferred,
        })
    }

    fn first_clause_ids(&self, fun: &Tree) -> Vec<SymbolId> {
        if fun.sym.is_none() {
            return Vec::new();
        }
        let s = self.st.get(fun.sym);
        if s.paramss.is_empty() {
            return s.params.clone();
        }
        match &fun.ty {
            Type::Method { paramss, .. } if paramss.len() < s.paramss.len() => {
                let drop = s.paramss.len() - paramss.len();
                s.paramss.get(drop).cloned().unwrap_or_default()
            }
            _ => s.paramss.first().cloned().unwrap_or_default(),
        }
    }

    pub(crate) fn named_arg_parts(arg: &Tree) -> Option<(String, Tree)> {
        if let TreeKind::Assign { lhs, rhs } = &arg.kind {
            if let TreeKind::Ident { name } = &lhs.kind {
                return Some((name.clone(), (**rhs).clone()));
            }
        }
        None
    }

    pub(crate) fn has_named_arg(args: &[Tree]) -> bool {
        args.iter().any(|a| Self::named_arg_parts(a).is_some())
    }

    /// A `new C` type tree that names `class_id` outright.
    ///
    /// The `copy` rewrites below build a constructor call, and spelling the
    /// class by *name* re-resolves it in whatever scope the rewrite happens
    /// to run in. `override def getDumpInfo = super.getDumpInfo.copy(mainInfo
    /// = …)` is written in files that never import `DumpInfo` -- it comes in
    /// through the inherited member -- and those reported `not found: type
    /// DumpInfo` with no position at all. nsc's `TypeTree(tp)`, the same
    /// marker `crate::materialize` uses.
    pub(crate) fn resolved_class_tpt(&self, class_id: SymbolId) -> Tree {
        let mut t = Tree::dummy(TreeKind::Ident {
            name: crate::materialize::RESOLVED_TYPE.to_string(),
        });
        t.ty = Type::Class {
            sym: class_id,
            args: Vec::new(),
        };
        t.sym = class_id;
        t
    }

    /// nsc's `NamesDefaults.removeNames`: place `name = value` arguments at
    /// their parameter positions.
    ///
    /// A named argument that already sits at its own position keeps later
    /// positional arguments legal (`f(a = 1, 2)` compiles); one that moves an
    /// argument makes every following positional argument an error. Returns one
    /// slot per parameter plus the positional overflow, which only a repeated
    /// final parameter can absorb.
    pub(crate) fn named_arg_slots(
        &mut self,
        args: Vec<Tree>,
        names: &[String],
    ) -> (Vec<Option<Tree>>, Vec<Tree>, bool) {
        let mut slots: Vec<Option<Tree>> = names.iter().map(|_| None).collect();
        // Parameter slot -> the position the argument now in it was written
        // at. `place_named_args` reads it back out to record what the
        // evaluation order has to be put back to; see `slot_source`.
        let mut slot_source: Vec<Option<usize>> = names.iter().map(|_| None).collect();
        let mut arg_pos: Vec<Option<usize>> = args.iter().map(|_| None).collect();
        let mut extra: Vec<Tree> = Vec::new();
        let mut positional_allowed = true;
        let mut ok = true;
        for (arg_index, a) in args.into_iter().enumerate() {
            let Some((n, rhs)) = Self::named_arg_parts(&a) else {
                if !positional_allowed {
                    self.error(a.span, "positional after named argument.");
                    ok = false;
                } else if arg_index < slots.len() {
                    arg_pos[arg_index] = Some(arg_index);
                    slot_source[arg_index] = Some(arg_index);
                    slots[arg_index] = Some(a);
                } else {
                    extra.push(a);
                }
                continue;
            };
            let Some(pos) = names.iter().position(|p| p == &n) else {
                self.error(a.span, format!("unknown parameter name: {n}"));
                ok = false;
                continue;
            };
            match arg_pos.iter().position(|p| *p == Some(pos)) {
                Some(prev) => {
                    self.error(
                        a.span,
                        format!(
                            "parameter '{n}' is already specified at parameter position {}",
                            prev + 1
                        ),
                    );
                    ok = false;
                }
                None => {
                    arg_pos[arg_index] = Some(pos);
                    slot_source[pos] = Some(arg_index);
                    slots[pos] = Some(rhs);
                }
            }
            if pos != arg_index {
                positional_allowed = false;
            }
        }
        self.slot_source = slot_source;
        (slots, extra, ok)
    }

    /// The 1-based index a `name$default$n` getter carries: nsc numbers
    /// defaults across *all* parameter clauses, so `f(a, b = 1)(c, d = 2)`
    /// gets `f$default$2` and `f$default$4`, not `$default$2` twice.
    fn default_getter_index(&self, fun: &Tree, param: SymbolId) -> usize {
        let sym = fun.sym;
        if sym.is_none() {
            return 1;
        }
        let s = self.st.get(sym);
        let flat: Vec<SymbolId> = if s.paramss.is_empty() {
            s.params.clone()
        } else {
            s.paramss.iter().flatten().copied().collect()
        };
        flat.iter().position(|&p| p == param).map_or(1, |i| i + 1)
    }

    /// Whether the callee is already erroneous, so named arguments cannot be
    /// resolved and any diagnostic here would only be a cascade.
    fn callee_is_erroneous(&self, fun: &Tree) -> bool {
        matches!(fun.ty, Type::Error) || (fun.sym.is_none() && fun.ty.is_no_type())
    }

    /// Drop the `name =` wrapper without reordering, so an argument whose
    /// parameter could not be resolved is not typed as an assignment to a
    /// non-existent variable.
    fn strip_named_args(args: &mut [Tree]) {
        for a in args.iter_mut() {
            if let Some((_, rhs)) = Self::named_arg_parts(a) {
                *a = rhs;
            }
        }
    }

    /// The first parameter clause of `m`, and whether it ends in a repeated
    /// parameter. A repeated parameter's *symbol* has type `Seq[T]` (its type
    /// inside the body), so only the method type still says `Repeated`.
    fn first_clause_of(&self, m: SymbolId) -> (Vec<SymbolId>, bool) {
        let s = self.st.get(m);
        let ids = if s.paramss.is_empty() {
            s.params.clone()
        } else {
            s.paramss.first().cloned().unwrap_or_default()
        };
        let repeated = match &s.ty {
            Type::Method { paramss, .. } => paramss
                .first()
                .and_then(|c| c.last())
                .is_some_and(|t| matches!(t, Type::Repeated(_))),
            _ => false,
        };
        (ids, repeated)
    }

    /// The types of the named arguments, typed speculatively: the diagnostics
    /// are rolled back and the call site's own trees are untouched, so this
    /// only serves to tell overloaded alternatives apart.
    fn probe_named_arg_types(&mut self, args: &[Tree]) -> Vec<(String, Type)> {
        let named: Vec<(String, Tree)> = args.iter().filter_map(Self::named_arg_parts).collect();
        let mark = self.diags.len();
        let mut out = Vec::with_capacity(named.len());
        for (name, mut rhs) in named {
            // A function literal needs an expected type to say anything useful.
            if matches!(rhs.kind, TreeKind::Function { .. }) {
                out.push((name, Type::NoType));
                continue;
            }
            self.type_expr(&mut rhs, &Type::NoType);
            out.push((name, rhs.ty.clone()));
        }
        self.diags.truncate(mark);
        out
    }

    /// The alternative among `alts` that declares every name the call site used
    /// — nsc narrows an overloaded callee by parameter name, then by argument
    /// type. `h(s: String, n: Int)` and `h(n: Int, s: String)` both declare
    /// `s` and `n`, so the types decide which one `h(n = 1, s = "x")` means.
    fn alt_for_named_args(
        &self,
        alts: &[SymbolId],
        named: &[(String, Type)],
        nargs: usize,
    ) -> Option<(Vec<SymbolId>, bool)> {
        let cands: Vec<(Vec<SymbolId>, bool)> = alts
            .iter()
            .filter(|&&m| self.st.get(m).kind == SymKind::Method)
            .map(|&m| self.first_clause_of(m))
            .filter(|(ids, _)| !ids.is_empty())
            .collect();
        let covers = |ids: &[SymbolId]| {
            named.iter().all(|(n, _)| {
                ids.iter()
                    .any(|i| self.st.get(*i).name.as_str() == n.as_str())
            })
        };
        let conforms = |ids: &[SymbolId]| {
            named.iter().all(|(n, t)| {
                if t.is_no_type() || t.is_error() {
                    return true;
                }
                match ids
                    .iter()
                    .find(|i| self.st.get(**i).name.as_str() == n.as_str())
                {
                    Some(&p) => self.arg_conforms(t, &self.st.get(p).ty, true, &[]),
                    None => false,
                }
            })
        };
        let pick = |f: &dyn Fn(&[SymbolId]) -> bool| -> Option<&(Vec<SymbolId>, bool)> {
            cands
                .iter()
                .find(|(ids, _)| ids.len() >= nargs && f(ids))
                .or_else(|| cands.iter().find(|(ids, _)| f(ids)))
        };
        pick(&|ids| covers(ids) && conforms(ids))
            .or_else(|| pick(&covers))
            .or_else(|| cands.first())
            .cloned()
    }

    /// A default spliced into a named-argument list is typed by the caller's
    /// own argument loop, which runs in the caller's scope. Type it here
    /// instead, where the scope the default was written in is still known, and
    /// let `NodeId::PRETYPED_DEFAULT` keep that loop off it. Defaults with no
    /// recorded scope (jar parameters) are left exactly as they were.
    fn pretype_spliced_default(&mut self, param: SymbolId, mut rhs: Tree) -> Tree {
        if !self.default_scopes.contains_key(&param) {
            return rhs;
        }
        let pty = self.st.get(param).ty.clone();
        // The expectation stays the call site's. A declared type that names a
        // type parameter is not one here: slick's
        // `case class Comprehension[+Fetch <: Option[Node]](…, fetch: Fetch =
        // None, …)` would be checking `None` against the bare `Fetch`, which
        // is a mismatch nsc never reports -- it is `Option[Node]` by the time
        // the call has settled what `Fetch` is. Only the *scope* moves here;
        // the fitting is left to the `adapt` in `type_expr`'s pretyped branch.
        let mut tps = Vec::new();
        collect_tparams(&pty, &mut tps);
        let pt = if tps.is_empty() { pty } else { Type::NoType };
        self.type_default_rhs_here(param, &mut rhs, &pt);
        rhs
    }

    /// Move each `name = value` into its parameter slot and fill the gaps left
    /// by omitted defaults. Shared by the method, constructor and `apply`
    /// paths; `defaults_inline` inlines a parameter's default expression
    /// instead of calling its `name$default$n` getter, which is what a
    /// constructor needs (there is no receiver yet at `new C(…)`).
    fn place_named_args(
        &mut self,
        args: &mut Vec<Tree>,
        fun: &Tree,
        ids: &[SymbolId],
        repeated_last: bool,
        defaults_inline: bool,
    ) -> bool {
        let names: Vec<String> = ids.iter().map(|id| self.st.get(*id).name.clone()).collect();
        let taken = std::mem::take(args);
        let (slots, extra, ok) = self.named_arg_slots(taken, &names);
        // Where each of these arguments was written. A by-name parameter is
        // struck out: its argument is not evaluated at the call site at all,
        // so lifting it into a `val` in front of the call would turn a thunk
        // into an eager evaluation.
        let mut order = std::mem::take(&mut self.slot_source);
        for (i, slot) in order.iter_mut().enumerate() {
            if ids.get(i).is_some_and(|p| {
                let s = self.st.get(*p);
                s.flags.contains(Flags::BYNAME) || matches!(s.ty, Type::ByName(_))
            }) {
                *slot = None;
            }
        }
        let last = slots.len().saturating_sub(1);
        let mut out = Vec::new();
        for (i, slot) in slots.into_iter().enumerate() {
            if let Some(t) = slot {
                out.push(t);
                continue;
            }
            let pid = ids[i];
            let flags = self.st.get(pid).flags;
            let default_rhs = self.st.get(pid).default_rhs.clone();
            if defaults_inline {
                if let Some(rhs) = default_rhs {
                    out.push(self.pretype_spliced_default(pid, rhs));
                    continue;
                }
            } else if flags.contains(Flags::DEFAULTPARAM) {
                let idx = self.default_getter_index(fun, pid);
                if let Some(filled) = self.default_getter_apply(fun, pid, idx, &out) {
                    out.push(filled);
                } else if let Some(rhs) = default_rhs {
                    out.push(self.pretype_spliced_default(pid, rhs));
                }
                continue;
            }
            if flags.contains(Flags::IMPLICIT) {
                // Leave a hole; `fill_defaults_and_implicits` searches for it.
                break;
            }
            if repeated_last && i == last {
                // `def f(a: Int, rest: Int*)` called as `f(a = 1)`.
                break;
            }
            // nsc reports one error per bad application; the missing slot is a
            // consequence of the name error already reported.
            if ok {
                self.error(
                    fun.span,
                    format!("missing argument for parameter `{}`", names[i]),
                );
            }
        }
        if repeated_last {
            out.extend(extra);
        } else if ok {
            for a in extra {
                self.error(a.span, "too many arguments");
            }
        }
        order.truncate(out.len());
        self.last_named_order = Some(order);
        *args = out;
        ok
    }

    /// `new C(b = 2, a = 1)`. Constructors are picked by argument type, so the
    /// names have to be resolved first — and against the overload that
    /// actually declares them.
    pub(crate) fn reorder_named_ctor_args(
        &mut self,
        args: &mut Vec<Tree>,
        class_id: Option<SymbolId>,
        fun: &Tree,
    ) -> bool {
        self.last_named_order = None;
        let Some(class_id) = class_id else {
            Self::strip_named_args(args);
            return true;
        };
        let alts = self.st.lookup_member(class_id, "<init>");
        let named = if alts.len() > 1 {
            self.probe_named_arg_types(args)
        } else {
            args.iter()
                .filter_map(|a| Self::named_arg_parts(a).map(|(n, _)| (n, Type::NoType)))
                .collect()
        };
        // `new C(1)(c = 3, b = 2)`: constructor arguments reach this path with
        // every clause flattened into one list (`flatten_curried_new`), so a
        // name has to be looked for across all of them. `first_clause_of` --
        // what `alt_for_named_args` uses, and right for a method, whose
        // clauses arrive one `Apply` at a time -- only ever saw `a`.
        let flat = {
            let ctor_alts: Vec<SymbolId> = alts
                .iter()
                .copied()
                .filter(|&m| self.st.get(m).kind == SymKind::Method)
                .collect();
            match ctor_alts.as_slice() {
                [only] if self.st.get(*only).paramss.len() > 1 => {
                    let s = self.st.get(*only);
                    let ids: Vec<SymbolId> = s.paramss.iter().flatten().copied().collect();
                    let repeated = match &s.ty {
                        Type::Method { paramss, .. } => paramss
                            .last()
                            .and_then(|c| c.last())
                            .is_some_and(|t| matches!(t, Type::Repeated(_))),
                        _ => false,
                    };
                    (!ids.is_empty()).then_some((ids, repeated))
                }
                _ => None,
            }
        };
        let (ids, repeated_last) = flat
            .or_else(|| self.alt_for_named_args(&alts, &named, args.len()))
            .unwrap_or_else(|| (self.st.get(class_id).ctor_fields.clone(), false));
        if ids.is_empty() {
            Self::strip_named_args(args);
            self.error(
                args.first().map(|a| a.span).unwrap_or(fun.span),
                "unimplemented syntax: named arguments (constructor parameters not resolved)",
            );
            return false;
        }
        self.place_named_args(args, fun, &ids, repeated_last, true)
    }

    /// The parameters to map named arguments onto, and whether the clause ends
    /// in a repeated parameter.
    fn named_arg_param_ids(&mut self, fun: &Tree, args: &[Tree]) -> (Vec<SymbolId>, bool) {
        if matches!(fun.ty, Type::Overload(_)) && !fun.sym.is_none() {
            let name = self.st.get(fun.sym).name.clone();
            let owner = self.st.get(fun.sym).owner;
            let mut alts = self.drop_overridden(self.st.lookup_member(owner, &name));
            if alts.is_empty() {
                alts = self.st.lookup(&name);
            }
            let named = self.probe_named_arg_types(args);
            let found = self.alt_for_named_args(&alts, &named, args.len());
            // `alt_for_named_args` answers with its first candidate when none
            // covers the names, so that the call reports one honest "unknown
            // parameter name". A hand-written prelude alternative is that case
            // without being that error: its parameters have no name a source
            // could write, and the library's pickle has the real ones.
            if let Some(found) = found {
                if self.ids_cover_named(&found.0, &named) {
                    return found;
                }
                for &a in &alts {
                    if let Some(ids) = self.prelude_alt_clause(a, args.len(), &named) {
                        return ids;
                    }
                }
                return found;
            }
            for &a in &alts {
                if let Some(ids) = self.prelude_alt_clause(a, args.len(), &named) {
                    return ids;
                }
            }
        }
        // `pkg.Bar(a = 1)`: `rewrite_receiver_apply` deliberately leaves a
        // qualified reference to a module (`fun.kind` a `Select`, `fun.ty` a
        // `Type::ModuleRef`) un-rewritten into `pkg.Bar.apply`, so codegen
        // keeps emitting a direct companion-apply call (see its doc comment).
        // `fun.sym` therefore names the module itself, not `apply`, and the
        // module carries no `paramss` of its own -- `first_clause_ids` below
        // would find nothing. Read the parameter names off the module's
        // `apply` member(s) instead, exactly as the `Overload` branch above
        // does for an ordinary overloaded callee.
        //
        // Which symbol carries those members depends on how the reference was
        // built. An `object`'s body is entered on its module *class* (`one$`),
        // and the module *value* (`one`) that a reference resolves to has no
        // members at all; `fun.ty` is the `ModuleRef` naming the class. Only a
        // reference whose symbol already *is* the module class -- which is what
        // a companion of a case class resolves to -- found anything here, so
        // every `html.dropdown(value, right = true)` in gitbucket's Twirl
        // templates reported "method parameters not resolved".
        if !fun.sym.is_none() && self.st.get(fun.sym).kind == SymKind::Module {
            let mut owners = vec![fun.sym];
            if let Type::ModuleRef(c) = &fun.ty {
                if *c != fun.sym {
                    owners.push(*c);
                }
            }
            for owner in owners {
                let alts = self.st.lookup_member(owner, "apply");
                if alts.is_empty() {
                    continue;
                }
                let named = self.probe_named_arg_types(args);
                if let Some(found) = self.alt_for_named_args(&alts, &named, args.len()) {
                    if self.ids_cover_named(&found.0, &named) {
                        return found;
                    }
                    for &a in &alts {
                        if let Some(ids) = self.prelude_alt_clause(a, args.len(), &named) {
                            return ids;
                        }
                    }
                    return found;
                }
                for &a in &alts {
                    if let Some(ids) = self.prelude_alt_clause(a, args.len(), &named) {
                        return ids;
                    }
                }
            }
        }
        let mut ids = self.first_clause_ids(fun);
        // A hand-written prelude method has parameter types and either no
        // parameter symbols or placeholder-named ones; the library's pickle
        // has the names (`crate::prelude_paramnames`).
        if !fun.sym.is_none() && !self.named_args_covered_by(&ids, args) {
            let remaining = match &fun.ty {
                Type::Method { paramss, .. } => Some(paramss.len()),
                _ => None,
            };
            let alt = self.prelude_clause_for(fun.sym, remaining);
            if !alt.is_empty() {
                ids = alt;
            }
        }
        // `fun.ty` may already have shed earlier clauses (`f(1)(b = 2)`), so
        // match the clause by length rather than taking the first.
        let repeated = match &fun.ty {
            Type::Method { paramss, .. } => paramss
                .iter()
                .find(|c| c.len() == ids.len())
                .and_then(|c| c.last())
                .is_some_and(|t| matches!(t, Type::Repeated(_))),
            _ => false,
        };
        (ids, repeated)
    }

    /// Remember, for the application node `id`, that its arguments no longer
    /// stand in the order they were written.
    ///
    /// Scala evaluates arguments left to right *as written*, and binds them to
    /// parameters by name afterwards (SLS 6.6.1), so an application whose names
    /// reorder it has to keep the written order observable. nsc does that by
    /// lifting the arguments into `val`s in front of the call
    /// (`NamesDefaults.transformNamedApplication`); here the same rewrite is
    /// [`crate::named_eval_order`], a pass over the typed tree, and this is
    /// what tells it which calls to look at. Nothing is recorded when the
    /// arguments happen to be in parameter order already, which is the common
    /// case (`f(a = 1, b = 2)`).
    pub(crate) fn record_named_arg_order(&mut self, id: NodeId) {
        let Some(order) = self.last_named_order.take() else {
            return;
        };
        // A synthesized application has no node id of its own to be keyed by.
        if id == NodeId(0) || id.is_filled_arg() || id.is_pretyped_default() {
            return;
        }
        let mut written = order.iter().flatten();
        let mut prev = match written.next() {
            Some(p) => *p,
            None => return,
        };
        let mut moved = false;
        for &p in written {
            if p < prev {
                moved = true;
            }
            prev = p;
        }
        if moved {
            self.st
                .named_arg_order
                .insert((self.file_index as u32, id.0), order);
        }
    }

    pub(crate) fn reorder_named_args(&mut self, args: &mut Vec<Tree>, fun: &Tree) -> bool {
        self.last_named_order = None;
        if !Self::has_named_arg(args) {
            return true;
        }
        let (ids, repeated_last) = self.named_arg_param_ids(fun, args);
        if ids.is_empty() {
            // Strip `name =` so the argument is not typed as an assignment to a
            // non-existent variable, which would bury the real error under a
            // "not found: value name" cascade.
            Self::strip_named_args(args);
            if !self.callee_is_erroneous(fun) {
                self.error(
                    args.first().map(|a| a.span).unwrap_or(fun.span),
                    "unimplemented syntax: named arguments (method parameters not resolved)",
                );
            }
            return false;
        }
        self.place_named_args(args, fun, &ids, repeated_last, false)
    }

    pub(crate) fn fill_defaults_and_implicits(
        &mut self,
        span: Span,
        args: &mut Vec<Tree>,
        param_tys: &[Type],
        fun: &Tree,
        pt: &Type,
    ) -> Option<Type> {
        let sym = fun.sym;
        if sym.is_none() {
            return None;
        }
        // `f(x)()` applies the *result*. `mk(3)` is a `() => Int`, so the
        // empty clause is `Function0.apply`, not a second parameter list of
        // `mk` -- but the tree still carries `mk`'s symbol (the callee is read
        // through the application), and reading its parameters here reported
        // "not enough arguments: expected 1, found 0" for a program scalac
        // accepts. Same test as erasure's `sym_denotes_callee`.
        if matches!(fun.ty, Type::Function { .. }) && matches!(fun.kind, TreeKind::Apply { .. }) {
            return None;
        }
        let fun_ty = &fun.ty;
        let s_paramss = self.st.get(sym).paramss.clone();
        let s_params = self.st.get(sym).params.clone();
        let paramss_ids: Vec<Vec<SymbolId>> = if !s_paramss.is_empty() {
            match fun_ty {
                Type::Method { paramss, .. } if paramss.len() < s_paramss.len() => {
                    let drop = s_paramss.len() - paramss.len();
                    s_paramss[drop..].to_vec()
                }
                _ => s_paramss.clone(),
            }
        } else if !s_params.is_empty() {
            vec![s_params]
        } else {
            return None;
        };
        self.implicit_undet_solved.clear();
        let first = paramss_ids.first().cloned().unwrap_or_default();
        // A repeated parameter accepts zero arguments (`count()`), so a call
        // that stops right before it is not short at all. Only this clause is
        // settled by that: the clauses after it still need filling, which is
        // what `f()(implicit …)` on `def f(xs: Int*)(implicit t: T)` needs.
        let short_first = args.len() < first.len()
            && !(first.len() - args.len() == 1
                && param_tys
                    .last()
                    .is_some_and(|t| matches!(t, Type::Repeated(_))));
        if short_first {
            let rest = first[args.len()..].to_vec();
            let all_implicit = rest
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT));
            let all_default = rest
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::DEFAULTPARAM));
            if all_implicit && !matches!(pt, Type::Method { .. } | Type::Function { .. }) {
                let off = args.len().min(param_tys.len());
                self.fill_implicit_params(span, args, &param_tys[off..], &rest);
            } else if all_default {
                for pid in rest.iter() {
                    let idx = self.default_getter_index(fun, *pid);
                    if let Some(filled) = self.default_getter_apply(fun, *pid, idx, args) {
                        args.push(filled);
                    } else if let Some(mut rhs) = self.st.get(*pid).default_rhs.clone() {
                        let pty = self.st.get(*pid).ty.clone();
                        self.type_default_rhs_here(*pid, &mut rhs, &pty);
                        args.push(rhs);
                    }
                }
            } else if !matches!(pt, Type::Method { .. } | Type::Function { .. }) {
                self.error(
                    span,
                    format!(
                        "not enough arguments: expected {}, found {}",
                        first.len(),
                        args.len()
                    ),
                );
            }
        }
        // How many clauses this call has already consumed. `paramss_ids` was
        // trimmed to the clauses still to come, so anything the declaration
        // has beyond that is behind us -- and the clause whose parameters
        // these arguments are being matched against is that one, not the
        // declaration's first. Reading the first clause here is what made
        // `def f[K, B](k: A => K)(g: A => B)(r: (B, B) => B)` solve `K` twice
        // and never `B`: `groupMapReduce`'s reduce function came out
        // `(Any, Any) => Any`.
        let clause_idx = s_paramss.len().saturating_sub(paramss_ids.len());
        if paramss_ids.len() > 1 {
            let rest_ids: Vec<SymbolId> = paramss_ids[1..].iter().flatten().copied().collect();
            let all_impl = !rest_ids.is_empty()
                && rest_ids
                    .iter()
                    .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT));
            if all_impl && !matches!(pt, Type::Method { .. } | Type::Function { .. }) {
                // Prefer the (possibly TypeApply-substituted) method type so
                // `mk[Int](2)` searches `ClassTag[Int]`, not raw `ClassTag[T]`.
                let rest_tys: Vec<Type> = match fun_ty {
                    Type::Method { paramss, .. } if paramss.len() > 1 => {
                        paramss[1..].iter().flatten().cloned().collect()
                    }
                    _ => rest_ids
                        .iter()
                        .map(|id| self.st.get(*id).ty.clone())
                        .collect(),
                };
                let rest_tys = if rest_tys.len() == rest_ids.len() {
                    rest_tys
                } else {
                    rest_ids
                        .iter()
                        .map(|id| self.st.get(*id).ty.clone())
                        .collect()
                };
                let rest_tys = self.instantiate_from_call(sym, clause_idx, &first, args, rest_tys);
                let rest_tys = self.solve_implicit_only_tparams(sym, rest_tys);
                self.fill_implicit_params(span, args, &rest_tys, &rest_ids);
                return None;
            }
            let rest_tys: Vec<Vec<Type>> = match fun_ty {
                Type::Method { paramss, .. } if paramss.len() > 1 => paramss[1..].to_vec(),
                _ => paramss_ids[1..]
                    .iter()
                    .map(|clause| {
                        clause
                            .iter()
                            .map(|id| self.st.get(*id).ty.clone())
                            .collect()
                    })
                    .collect(),
            };
            let rest_tys: Vec<Vec<Type>> = rest_tys
                .into_iter()
                .map(|tys| self.instantiate_from_call(sym, clause_idx, &first, args, tys))
                .collect();
            let ret = match fun_ty {
                Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
                _ => match &self.st.get(sym).ty {
                    Type::Method { ret, .. } => (**ret).clone(),
                    _ => Type::NoType,
                },
            };
            let ret = self
                .instantiate_from_call(sym, clause_idx, &first, args, vec![ret])
                .into_iter()
                .next()
                .unwrap_or(Type::NoType);
            return Some(Type::Method {
                paramss: rest_tys,
                ret: Box::new(ret),
            });
        }
        None
    }

    /// nsc's undetermined type parameters, for a call whose value arguments
    /// left some of the callee's parameters open: `mk(s)` on
    /// `def mk[T: TT](s: String): Seq[Int] => Rep[T]` mentions `T` only in the
    /// implicit clause, so the witness is the only thing that can pin it down
    /// (`SimpleFunction.nullary` in slick relies on it). Solve them from the
    /// implicit parameter types and substitute; when the search cannot settle
    /// every one of them, leave the types as they were and let the ordinary
    /// "could not find implicit value" diagnostic describe what happened.
    fn solve_implicit_only_tparams(&mut self, sym: SymbolId, rest_tys: Vec<Type>) -> Vec<Type> {
        let undet: Vec<SymbolId> = self
            .st
            .get(sym)
            .tparams
            .iter()
            .copied()
            .filter(|tp| rest_tys.iter().any(|t| type_mentions_tparam_deep(t, *tp)))
            .collect();
        if undet.is_empty() {
            return rest_tys;
        }
        // `undet_solution` searches under an immutable borrow and cannot load
        // a companion itself. Without this the search for `LazyZip2.map`'s
        // `BuildFrom[C1, B, C]` ran against a scope that did not yet hold
        // `BuildFrom`'s companion at all.
        for t in rest_tys.clone() {
            self.warm_implicit_scope(&t);
        }
        let mut solved = self.undet_solution(&rest_tys, &undet);
        if solved.is_none() && self.warm_implicit_candidates(&rest_tys) {
            // The witness may be a jar class whose parents nothing had read:
            // `implicit F: Async[F]` answers `GenTemporal[F, E]` only through
            // `Async extends … GenTemporal[F, Throwable]`, and that is what
            // says `E = Throwable`. Left unsolved, the parameter reached
            // `fill_implicit_params` as `GenTemporal[F, _]` and no candidate
            // could match it (slick's `slick/basic/ConcurrencyControl.scala`).
            solved = self.undet_solution(&rest_tys, &undet);
        }
        if solved.is_none()
            && rest_tys.iter().any(|ty| {
                matches!(ty, Type::Class { sym, .. } if self.st.get(*sym).jvm_name == "scala/reflect/ClassTag")
            })
        {
            // This continuation is needed only for compiler-generated tags.
            // Ordinary unsuccessful searches must not be repeated here.
            // Keep bindings obtained from earlier witnesses even when a later
            // clause needs compiler-generated ClassTag evidence.
            let mut partial: Vec<(SymbolId, Type)> = Vec::new();
            let mut ambiguous = false;
            for ty in &rest_tys {
                let ids: Vec<_> = partial.iter().map(|(tp, _)| *tp).collect();
                let vals: Vec<_> = partial.iter().map(|(_, ty)| ty.clone()).collect();
                let want = crate::symbol::subst_tparams_slice(&ids, &vals, ty);
                let open: Vec<_> = undet
                    .iter()
                    .copied()
                    .filter(|tp| !ids.contains(tp))
                    .collect();
                let (found, bindings) = self.search_implicit_undet(&want, &open, 0);
                if matches!(found, ImplicitSearch::Ambiguous(_)) {
                    ambiguous = true;
                    break;
                }
                if found.is_found() {
                    partial.extend(bindings);
                }
            }
            if !ambiguous {
                for tp in &undet {
                    if partial.iter().any(|(id, _)| id == tp)
                        || self.tparam_in_scope(*tp)
                        || !self.st.get(*tp).tparams.is_empty()
                    {
                        continue;
                    }
                    // Failed ordinary evidence must retain its open variables
                    // in the diagnostic. Minimize only variables occurring
                    // exclusively in ClassTag requests, before materialization.
                    let only_tags = rest_tys.iter().filter(|ty| type_mentions_tparam_deep(ty, *tp)).all(|ty| {
                        matches!(ty, Type::Class { sym, .. } if self.st.get(*sym).jvm_name == "scala/reflect/ClassTag")
                    });
                    if only_tags {
                        let ids: Vec<_> = partial.iter().map(|(tp, _)| *tp).collect();
                        let vals: Vec<_> = partial.iter().map(|(_, ty)| ty.clone()).collect();
                        let lo = self.st.get(*tp).bound_lo.clone().unwrap_or(Type::Nothing);
                        partial.push((*tp, crate::symbol::subst_tparams_slice(&ids, &vals, &lo)));
                    }
                }
                if !partial.is_empty() {
                    solved = Some(partial);
                }
            }
        }
        let Some(sol) = solved else {
            return rest_tys;
        };
        let ids: Vec<SymbolId> = sol.iter().map(|(i, _)| *i).collect();
        let ts: Vec<Type> = sol.iter().map(|(_, t)| t.clone()).collect();
        let out = rest_tys
            .iter()
            .map(|t| crate::symbol::subst_tparams_slice(&ids, &ts, t))
            .collect();
        self.implicit_undet_solved = sol;
        out
    }

    fn instantiate_from_call(
        &self,
        sym: SymbolId,
        clause_idx: usize,
        first: &[SymbolId],
        args: &[Tree],
        tys: Vec<Type>,
    ) -> Vec<Type> {
        if self.st.get(sym).tparams.is_empty() || tys.is_empty() {
            return tys;
        }
        // The method type keeps `Repeated`; the parameter symbols carry `Seq`
        // (their type inside the body), which would not unify with an argument.
        let sig_first: Vec<Type> = match &self.st.get(sym).ty {
            Type::Method { paramss, .. } => paramss.get(clause_idx).cloned().unwrap_or_default(),
            _ => Vec::new(),
        };
        let orig_first: Vec<Type> = if sig_first.len() == first.len() {
            sig_first
        } else {
            first.iter().map(|id| self.st.get(*id).ty.clone()).collect()
        };
        // By-name params are adapted to `() => T`. Infer `T`, not Function0,
        // so later clauses see `R2` rather than `() => R2`.
        let orig_for_infer: Vec<Type> = orig_first
            .iter()
            .map(|p| match p {
                Type::ByName(inner) => (**inner).clone(),
                other => other.clone(),
            })
            .collect();
        let arg_tys: Vec<Type> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if matches!(orig_first.get(i), Some(Type::ByName(_))) {
                    unwrap_fn0_or_byname(&a.ty)
                } else {
                    a.ty.clone()
                }
            })
            .collect();
        let inst = self.infer_method_tparams(sym, &orig_for_infer, &arg_tys);
        if inst.is_empty() {
            return tys;
        }
        let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
        let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
        tys.iter()
            .map(|t| crate::symbol::subst_tparams_slice(&tps, &args_t, t))
            .collect()
    }

    fn default_getter_apply(
        &mut self,
        fun: &Tree,
        param: SymbolId,
        index_1based: usize,
        prior: &[Tree],
    ) -> Option<Tree> {
        let meth = fun.sym;
        if meth.is_none() {
            return None;
        }
        let mname = self.st.get(meth).name.clone();
        // A case class's synthetic `apply` keeps splicing the default's stored
        // expression. Its `apply$default$n` getter exists (nsc's, declared by
        // `crate::ctor_defaults`) but only so a *separately compiled* caller
        // can link; calling it from here would fix the argument's type before
        // the class's type parameters are solved, and nsc infers them from the
        // getter's result instead. slick's `case class Comprehension[+Fetch <:
        // Option[Node]](…, fetch: Fetch = None, …)` is exactly that: the
        // getter answers `None$`, which does not conform to `Fetch` until the
        // call site has chosen `Fetch = None.type`.
        let meth_flags = self.st.get(meth).flags;
        if mname == "apply"
            && meth_flags.contains(Flags::CASE)
            && meth_flags.contains(Flags::SYNTHETIC)
        {
            return None;
        }
        let gname = format!("{mname}$default${index_1based}");
        // The pickle spells the constructor getter `<init>$default$n`, while
        // its JVM classfile method is `$lessinit$greater$default$n`.
        let jvm_ctor_gname = format!("$lessinit$greater$default${index_1based}");
        let owner = self.st.get(meth).owner;
        let span = fun.span;
        if mname == "<init>" {
            self.ensure_classfile_members_loaded(owner, &jvm_ctor_gname, span);
        }
        let lookup_getter = |st: &crate::symbol::SymbolTable, owner: SymbolId, name: &str| {
            st.lookup_member(owner, name)
                .into_iter()
                .find(|&id| st.get(id).kind == crate::symbol::SymKind::Method)
        };
        let mut getter_owner = owner;
        let mut companion_module = None;
        let mut gid = if mname == "<init>" {
            // Prefer the JVM spelling when the classfile was available. The
            // pickle spelling is retained as a fallback for a source class
            // whose getter is synthesized by this compiler.
            lookup_getter(&self.st, owner, &jvm_ctor_gname)
                .or_else(|| lookup_getter(&self.st, owner, &gname))
        } else {
            lookup_getter(&self.st, owner, &gname)
        };
        if gid.is_none() && mname == "<init>" {
            // `-Xno-forwarders` removes the static bridge from the class, and
            // nested classes never get that bridge even in an ordinary nsc
            // build.  Their real getter is an instance method on `C$` (or
            // `Outer$C$`), so load the companion and select it through its
            // module value just as Scala source does.
            self.load_companion_module(owner);
            if let Some(module) = self.st.companion_module(owner) {
                let mcls = self.st.module_class_of(module);
                gid = lookup_getter(&self.st, mcls, &jvm_ctor_gname)
                    .or_else(|| lookup_getter(&self.st, mcls, &gname));
                if gid.is_some() {
                    getter_owner = mcls;
                    companion_module = Some(module);
                }
            }
        }
        let gid = gid?;
        let getter_name = self.st.get(gid).name.clone();
        // An *inserted* `apply` names the receiver itself, not a member of it:
        // `Outer.Inner.Nested(2)` is `Select(Outer.Inner, "Nested")` carrying
        // the companion's `apply` as its symbol, so `method_receiver`'s
        // "take the qualifier" answers `Outer.Inner` and the getter call came
        // out as `Inner$.apply$default$2`. The head of the chain is the
        // receiver in that case; it is re-typed from scratch because the tree
        // in hand is already typed as the *method*.
        let head = Self::application_head(fun);
        let inserted_apply =
            mname == "apply" && Self::head_name(head).is_some_and(|n| n != "apply");
        // A separately compiled plain class exposes primary-constructor
        // defaults as static `$lessinit$greater$default$n` methods on the
        // class itself.  The synthetic `<init>` tree has no ordinary method
        // receiver, so `method_receiver` manufactured `this`; typing that
        // receiver in an `extends Parent(...)` clause is invalid and nsc's
        // static getter call never needs it.  Qualify the getter with the
        // class name so selection resolves the static member and codegen
        // emits `invokestatic` without loading a receiver.
        let ctor_default_getter = (self.st.get(meth).flags.contains(Flags::CONSTRUCTOR)
            || self.st.get(meth).name == "<init>")
            && getter_owner == owner
            && getter_name.starts_with("$lessinit$greater$default$");
        // The classfile getter is a static forwarder. Preserve that fact for
        // selection/codegen even when the pickle supplied its symbol without
        // JVM access flags.
        if ctor_default_getter {
            let f = self.st.get(gid).flags.with(Flags::STATIC);
            self.st.get_mut(gid).flags = f;
        }
        let ctor_static_getter = ctor_default_getter;
        let ctor_companion_getter = companion_module.is_some();
        let recv = if ctor_static_getter {
            let owner_name = self.st.get(owner).name.clone();
            Tree {
                id: NodeId(0),
                span,
                kind: TreeKind::Ident { name: owner_name },
                ty: Type::Class {
                    sym: owner,
                    args: self
                        .st
                        .get(owner)
                        .tparams
                        .iter()
                        .map(|&t| Type::TypeParam(t))
                        .collect(),
                },
                sym: owner,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            }
        } else if ctor_companion_getter {
            let module = companion_module.expect("companion getter has module");
            let module_name = self.st.get(module).name.clone();
            Tree {
                id: NodeId(0),
                span,
                kind: TreeKind::Ident { name: module_name },
                ty: Type::ModuleRef(self.st.module_class_of(module)),
                sym: module,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            }
        } else if inserted_apply {
            let mut r = head.clone();
            r.id = NodeId(0);
            r.ty = Type::NoType;
            r.sym = SymbolId::NONE;
            self.type_expr(&mut r, &Type::NoType);
            r
        } else {
            self.method_receiver(fun)
        };
        let mut preceding = Self::applied_clause_args(fun);
        preceding.extend_from_slice(prior);
        // The getter's own arity is the truth, not the number of arguments
        // that precede the default. scalac emits a *nullary* getter whenever
        // the default does not read an earlier parameter --
        // `SeqOps.lastIndexOf$default$2()` takes nothing though `elem` comes
        // first, and so does
        // `ReificationSupportApi.SyntacticTermIdent.apply$default$2()`. Passing
        // arguments a nullary getter does not declare emits a call that cannot
        // link.
        let want = self.st.get(gid).paramss.iter().flatten().count();
        preceding.truncate(want);
        let preceding = &preceding[..];
        let param_ty = self.default_param_type(fun, param);
        let mut gfun = Tree {
            id: NodeId(0),
            span,
            // Keep the class qualifier on a static constructor getter.  Its
            // type and symbol are already resolved above, so `type_select`
            // can inspect the real classfile member without resolving the
            // class name as its companion object.  Going through Select is
            // essential: the normal Apply path then checks the getter's
            // descriptor and adapts its result to the constructor parameter,
            // instead of trusting the parameter type as the getter result.
            kind: TreeKind::Select {
                qual: Box::new(recv),
                name: getter_name,
            },
            ty: self.st.get(gid).ty.clone(),
            sym: gid,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        self.type_expr(&mut gfun, &Type::NoType);
        let mut call = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(gfun),
                args: preceding.to_vec(),
            },
            ty: param_ty.clone(),
            sym: gid,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        self.type_expr(&mut call, &param_ty);
        Some(call)
    }

    /// The parameter type as seen by this particular call.  A constructor
    /// selected through `extends Parent[String]` carries `String` in its
    /// substituted method type even though the parameter symbol itself still
    /// has the declaration's `T`; using the latter would reject a valid
    /// numeric default and accept an invalid String default alike.
    fn default_param_type(&self, fun: &Tree, param: SymbolId) -> Type {
        let method = fun.sym;
        if method.is_none() {
            return self.st.get(param).ty.clone();
        }
        let decl = self.st.get(method);
        let mut flat = 0usize;
        for (ci, clause) in decl.paramss.iter().enumerate() {
            for (pi, &pid) in clause.iter().enumerate() {
                if pid != param {
                    flat += 1;
                    continue;
                }
                if let Type::Method { paramss, .. } = &fun.ty {
                    // `fun.ty` contains only the clauses left after earlier
                    // Apply nodes. Match the declaration's clause index to
                    // that remaining suffix before reading the expected type.
                    // For `slice(a)(b = 0)(session)`, the default for b is in
                    // clause zero of fun.ty after slice(a), not clause one.
                    let consumed = decl.paramss.len().saturating_sub(paramss.len());
                    if let Some(ty) = ci
                        .checked_sub(consumed)
                        .and_then(|remaining| paramss.get(remaining))
                        .and_then(|clause| clause.get(pi))
                    {
                        return ty.clone();
                    }
                    // Constructor matching flattens curried clauses, while
                    // the stored method type retains them.  Accommodate the
                    // flattened view used by parent/default filling too.
                    if paramss.len() == 1 {
                        if let Some(ty) = paramss[0].get(flat) {
                            return ty.clone();
                        }
                    }
                }
                return self.st.get(param).ty.clone();
            }
        }
        self.st.get(param).ty.clone()
    }

    /// The arguments of the parameter clauses already applied to `fun`. A
    /// `name$default$n` getter for a later clause takes all of them
    /// (`def f(a: Int)(b: Int = a)` gives `f$default$2(a: Int)`).
    fn applied_clause_args(fun: &Tree) -> Vec<Tree> {
        match &fun.kind {
            TreeKind::Apply { fun, args } => {
                let mut v = Self::applied_clause_args(fun);
                v.extend(args.iter().cloned());
                v
            }
            TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
                Self::applied_clause_args(fun)
            }
            _ => Vec::new(),
        }
    }

    /// The head of an application chain: `f(a)(b)` and `f[T](a)` both give `f`.
    fn application_head(fun: &Tree) -> &Tree {
        match &fun.kind {
            TreeKind::Apply { fun, .. }
            | TreeKind::TypeApply { fun, .. }
            | TreeKind::Typed { expr: fun, .. } => Self::application_head(fun),
            _ => fun,
        }
    }

    /// The name a chain head selects, when it selects one at all.
    fn head_name(t: &Tree) -> Option<&str> {
        match &t.kind {
            TreeKind::Select { name, .. } | TreeKind::Ident { name } => Some(name.as_str()),
            _ => None,
        }
    }

    fn method_receiver(&self, fun: &Tree) -> Tree {
        match &fun.kind {
            TreeKind::Apply { fun, .. }
            | TreeKind::TypeApply { fun, .. }
            | TreeKind::Typed { expr: fun, .. } => self.method_receiver(fun),
            TreeKind::Select { qual, .. } => (**qual).clone(),
            _ => {
                let this_ty = if self.st.this_class.is_none() {
                    Type::NoType
                } else {
                    Type::ModuleRef(self.st.this_class)
                };
                Tree {
                    id: NodeId(0),
                    span: fun.span,
                    kind: TreeKind::This { qual: None },
                    ty: this_ty,
                    sym: self.st.this_class,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                }
            }
        }
    }

    /// The tree for a resolved implicit. A derivation rule
    /// (`implicit def listShow[A](implicit s: Show[A]): Show[List[A]]`) is
    /// applied to its own implicits, which are resolved the same way.
    pub(crate) fn implicit_tree(
        &mut self,
        id: SymbolId,
        pt: &Type,
        span: Span,
        depth: usize,
    ) -> Tree {
        let (paramss, ret) = match self.implicit_candidate_ty(id).into_owned() {
            Type::Method { paramss, ret } => (paramss, (*ret).clone()),
            _ => return self.ref_implicit(id, span),
        };
        let tps = self.st.get(id).tparams.clone();
        // The solved type arguments of a polymorphic implicit
        // (`<:<.refl[A]` fitted to `Int <:< Any` gives `A = Int`), so the tree
        // carries the instantiated type rather than the declared `=:=[A, A]`.
        let targs = self
            .implicit_fit_at(id, pt, depth, &[])
            .map(|f| f.targs)
            .or_else(|| self.implicit_targs(id, &ret, pt))
            .unwrap_or_default();
        if paramss.iter().all(|c| c.is_empty()) || depth >= crate::implicits::MAX_IMPLICIT_DEPTH {
            let mut t = self.ref_implicit(id, span);
            if targs.len() == tps.len() && !tps.is_empty() {
                t.ty = crate::symbol::subst_tparams_slice(&tps, &targs, &ret);
            }
            return t;
        }
        let inst = |t: &Type| -> Type {
            if targs.len() == tps.len() && !tps.is_empty() {
                crate::symbol::subst_tparams_slice(&tps, &targs, t)
            } else {
                t.clone()
            }
        };
        // Through `ref_implicit`, not as a bare `Ident`: a witness declared by
        // a trait its companion object only *mixes in* has to be named through
        // that object. `BuildFrom.buildFromSortedSetOps` is declared in
        // `BuildFromLowPriority1` and takes an `Ordering`, which is exactly
        // the case this branch handles -- emitted bare, codegen loaded `this`
        // and cast it: `class Main$ cannot be cast to class
        // BuildFromLowPriority1` from a program that type-checked.
        let mut tree = self.ref_implicit(id, span);
        tree.ty = inst(&ret);
        for clause in &paramss {
            let mut cargs = Vec::with_capacity(clause.len());
            for p in clause {
                let want = inst(p);
                self.warm_implicit_scope(&want);
                match self.search_implicit(&want) {
                    ImplicitSearch::Found(inner) => {
                        cargs.push(self.implicit_tree(inner, &want, span, depth + 1))
                    }
                    // A tag is *built*, not found. `fill_implicit_params` has
                    // always known that; this recursion did not, so a rule
                    // reached through another implicit could not have a
                    // `ClassTag` parameter of its own even though
                    // `implicitly[ClassTag[Seq[Any]]]` written out compiled.
                    _ if self.classtag_apply_fallback(&want, span).is_some()
                        || crate::materialize::tag_request(&self.st, &want).is_some() =>
                    {
                        match self.classtag_apply_fallback(&want, span) {
                            Some(t) => cargs.push(t),
                            None => match self.materialize_tag(&want, span) {
                                Some(t) => cargs.push(t),
                                None => {
                                    let diverged = self.diverged_implicit.borrow().clone();
                                    self.error(
                                        span,
                                        self.missing_implicit_message(&want, diverged),
                                    );
                                    return tree;
                                }
                            },
                        }
                    }
                    _ => {
                        let diverged = self.diverged_implicit.borrow().clone();
                        self.error(span, self.missing_implicit_message(&want, diverged));
                        return tree;
                    }
                }
            }
            let ty = tree.ty.clone();
            tree = Tree {
                id: scala_rs_parser::NodeId(0),
                span,
                kind: TreeKind::Apply {
                    fun: Box::new(tree),
                    args: cargs,
                },
                ty,
                sym: id,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            };
        }
        tree.ty = inst(&ret);
        tree
    }

    pub(crate) fn fill_implicit_params(
        &mut self,
        span: Span,
        args: &mut Vec<Tree>,
        param_tys: &[Type],
        rest: &[SymbolId],
    ) {
        let filled_from = args.len();
        self.fill_implicit_params_in(span, args, param_tys, rest);
        // Mark what this pass added, so a re-typing of the same application
        // (`retry_tupled_args`) starts from the arguments the user wrote.
        for a in args[filled_from..].iter_mut() {
            a.id = NodeId::FILLED_ARG;
        }
    }

    fn fill_implicit_params_in(
        &mut self,
        span: Span,
        args: &mut Vec<Tree>,
        param_tys: &[Type],
        rest: &[SymbolId],
    ) {
        for (i, pid) in rest.iter().enumerate() {
            let pty = param_tys
                .get(i)
                .cloned()
                .unwrap_or_else(|| self.st.get(*pid).ty.clone());
            self.warm_implicit_scope(&pty);
            let mut search = self.search_implicit(&pty);
            if matches!(search, ImplicitSearch::None)
                && self.warm_implicit_candidates(std::slice::from_ref(&pty))
            {
                search = self.search_implicit(&pty);
            }
            match search {
                ImplicitSearch::Found(id) => {
                    let mut r = self.implicit_tree(id, &pty, span, 0);
                    self.adapt(&mut r, &pty);
                    args.push(r);
                }
                ImplicitSearch::None => {
                    // Read the divergence record before the fallbacks run their
                    // own searches and reset it.
                    let diverged = self.diverged_implicit.borrow().clone();
                    if let Some(ct) = self.classtag_apply_fallback(&pty, span) {
                        args.push(ct);
                    } else if let Some(lam) = self.identity_view(&pty, span) {
                        args.push(lam);
                    } else if let Some(lam) = self.array_wrap_view(&pty, span) {
                        args.push(lam);
                    // SLS 7.2: an implicit parameter of type `A => B` is a
                    // view request, and an `implicit def` answers it
                    // eta-expanded (see `views.rs`).
                    } else if let Some(lam) = self.conversion_view(&pty, span) {
                        args.push(lam);
                    // nsc does not report a missing `TypeTag[T]` either: it
                    // expands `materializeTypeTag` (`crate::materialize`).
                    } else if let Some(tag) = self.materialize_tag(&pty, span) {
                        args.push(tag);
                    } else if let Some(d) = self.implicit_param_default(*pid, &pty) {
                        args.push(d);
                    } else {
                        self.error(span, self.missing_implicit_message(&pty, diverged));
                    }
                }
                ImplicitSearch::Ambiguous(ids) => {
                    self.error(
                        span,
                        format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                    );
                }
            }
        }
    }

    /// An implicit parameter that carries a default falls back to it when the
    /// search comes up empty; nsc reports a missing implicit only for a
    /// parameter that has nothing to fall back on.
    ///
    /// slick's `ScalaBaseType` is written around exactly that:
    /// `def apply[T](implicit classTag: ClassTag[T],
    ///               ordering: Ordering[T] = null): ScalaBaseType[T]`,
    /// called as `ScalaBaseType[T]` for an abstract `T` and for `Null`. The
    /// body is typed where it was written (`type_default_rhs_here`), so the
    /// fallback is the declaration's expression, not something re-resolved in
    /// the caller's scope.
    fn implicit_param_default(&mut self, param: SymbolId, pty: &Type) -> Option<Tree> {
        if !self.st.get(param).flags.contains(Flags::DEFAULTPARAM) {
            return None;
        }
        let mut rhs = self.st.get(param).default_rhs.clone()?;
        self.type_default_rhs_here(param, &mut rhs, pty);
        Some(rhs)
    }

    /// Whether a type has an *erasure* nsc's `ClassTag` materialiser can turn
    /// into a `classOf`.
    ///
    /// `Implicits.manifestOfType` with `full = false` builds the tag out of
    /// that erasure: a class becomes `classOf[C]` however many type arguments
    /// it carries. A type whose erasure is not a class has no tag of its own,
    /// and unless the scope supplies one the implicit search fails — that is
    /// the whole of `No ClassTag available for T`.
    ///
    /// Probed against scalac 2.13.16. Rejected: a method's own type parameter
    /// (`def f[T] = classTag[T]`), one with an upper bound (`T <: String`), a
    /// class's type parameter, an abstract `type T` member, `Array[T]`,
    /// `({ type L = T })#L`, and `CC[A]` for a higher-kinded parameter `CC`.
    /// Accepted: `Int`, `String`, `Any`, `Null`, `Nothing`, `Unit`,
    /// `Array[Int]`, `List[T]`, `Map[T, T]`, `List[_]`, `T with AnyRef` and a
    /// singleton `P.type`.
    fn classtag_erasable(&self, t: &Type) -> bool {
        match t {
            // No erasure of its own: the tag has to come from the scope.
            // Only a parameter the source can still *name* is abstract here.
            // nsc instantiates a call's undetermined parameters before it
            // asks for the tag — `bar(Array(): _*)` is `Array[Nothing]()`,
            // and `ClassTag.Nothing` answers that — while our inference
            // leaves the callee's own `Type::TypeParam` in place. Refusing
            // those cost `pos/t3859`, `pos/t5692c` and `pos/t5859`.
            Type::TypeParam(s) => !self.tparam_in_scope(*s),
            Type::TypeMember(_) => false,
            Type::Class { sym, .. } => match self.st.get(*sym).kind {
                SymKind::TypeParam => !self.tparam_in_scope(*sym),
                SymKind::TypeMember => false,
                _ => true,
            },
            Type::Array(e)
            | Type::ByName(e)
            | Type::Repeated(e)
            | Type::Annotated { tpe: e, .. } => self.classtag_erasable(e),
            Type::Applied { ctor, .. } => self.classtag_erasable(ctor),
            // nsc erases an intersection to `intersectionDominator`, which
            // prefers a parent that is a class over one that is not — so
            // `T with AnyRef` is tagged as `Object` and only an intersection
            // of nothing but abstract types has no tag. (scalac accepts
            // `classTag[T with AnyRef]`; the first attempt here refused it.)
            Type::Refined { parents, .. } => {
                parents.is_empty() || parents.iter().any(|p| self.classtag_erasable(p))
            }
            // Everything else — a value type, a function, a tuple, a
            // singleton, and also an unresolved `Named` or an `Error` — keeps
            // the old behaviour. Refusing on an error type would only add a
            // second diagnostic to a program that already has one.
            _ => true,
        }
    }

    /// nsc's `findSubManifest`: the tag for an array's element type, taken
    /// from the implicit scope first and built only if the scope has none.
    /// It is a whole implicit search in nsc, which is why a context bound
    /// two array levels down still answers.
    fn classtag_sub(&self, ct_cls: SymbolId, t: &Type, span: Span) -> Option<Tree> {
        let want = Type::Class {
            sym: ct_cls,
            args: vec![t.clone()],
        };
        if let ImplicitSearch::Found(id) = self.search_implicit(&want) {
            let r = self.ref_implicit(id, span);
            // A witness that still wants arguments of its own is left to the
            // caller's `implicit_tree`; only a plain value is spliced here.
            if !matches!(r.ty, Type::Method { .. }) {
                return Some(r);
            }
        }
        self.classtag_tree(ct_cls, t, span)
    }

    /// `<tag>.wrap`, which is `ClassTag(ScalaRunTime.arrayClass(<tag>
    /// .runtimeClass))` — the same tag nsc's `arrayType` factory builds.
    fn classtag_wrap(
        &self,
        ct_cls: SymbolId,
        inner: Tree,
        elem: &Type,
        span: Span,
    ) -> Option<Tree> {
        let wrap = self
            .st
            .lookup_member(ct_cls, "wrap")
            .into_iter()
            .find(|&id| self.st.get(id).kind == SymKind::Method)?;
        Some(Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(inner),
                name: "wrap".into(),
            },
            ty: Type::Class {
                sym: ct_cls,
                args: vec![Type::Array(Box::new(elem.clone()))],
            },
            sym: wrap,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        })
    }

    /// The tree nsc's materialiser builds for `ClassTag[t]`, or `None` when
    /// it can build none — in which case the search has failed and the caller
    /// reports `No ClassTag available for t`.
    fn classtag_tree(&self, ct_cls: SymbolId, t: &Type, span: Span) -> Option<Tree> {
        // Standard tags have canonical values; select them only after the
        // requested type is known, rather than registering them as implicits.
        let canonical = match t {
            Type::Int => Some("Int"),
            Type::Long => Some("Long"),
            Type::Double => Some("Double"),
            Type::Float => Some("Float"),
            Type::Boolean => Some("Boolean"),
            Type::Byte => Some("Byte"),
            Type::Short => Some("Short"),
            Type::Char => Some("Char"),
            Type::Unit => Some("Unit"),
            Type::Any => Some("Any"),
            Type::AnyRef => Some("AnyRef"),
            Type::Nothing => Some("Nothing"),
            Type::Null => Some("Null"),
            _ => None,
        };
        if let Some(name) = canonical {
            let module = self.st.companion_module(ct_cls)?;
            let owner = self.st.module_class_of(module);
            if let Some(id) = self.st.lookup_member(owner, name).into_iter().next() {
                let mut tree = self.ref_implicit(id, span);
                tree.ty = Type::Class {
                    sym: ct_cls,
                    args: vec![t.clone()],
                };
                return Some(tree);
            }
        }
        // `Array[E]` where `E` has no erasure of its own: nsc emits
        // `arrayType(findSubManifest(E))`, not a `classOf` of the array. The
        // difference is visible — `def f[T: ClassTag] = classTag[Array[T]]`
        // must report `[[I` at `T = Int`, and a `classOf` of the array type
        // reported `int`. `src/library/scala/Array.scala`'s `ofDim` is eleven
        // of these.
        if let Type::Array(e) = t {
            if !self.classtag_erasable(e) {
                let inner = self.classtag_sub(ct_cls, e, span)?;
                return self.classtag_wrap(ct_cls, inner, e, span);
            }
        }
        if !self.classtag_erasable(t) {
            return None;
        }
        self.classtag_apply_tree(ct_cls, t, span)
    }

    /// nsc fills `ClassTag[String]` via `ClassTag.apply(classOf[String])` when
    /// there is no primitive getter (`ClassTag.Int`, …).
    pub(crate) fn classtag_apply_fallback(&self, pt: &Type, span: Span) -> Option<Tree> {
        let Type::Class { sym, args } = pt else {
            return None;
        };
        if self.st.get(*sym).name != "ClassTag" || args.is_empty() {
            return None;
        }
        self.classtag_tree(*sym, &args[0], span)
    }

    fn classtag_apply_tree(&self, ct_cls: SymbolId, t: &Type, span: Span) -> Option<Tree> {
        let elem = t.clone();
        let module = self.st.companion_module(ct_cls)?;
        let mcls = self.st.module_class_of(module);
        let apply = self
            .st
            .lookup_member(mcls, "apply")
            .into_iter()
            .find(|&id| self.st.get(id).kind == crate::symbol::SymKind::Method)?;
        let class_arg = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "$classOf".into(),
            },
            ty: elem,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let recv = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "ClassTag".into(),
            },
            ty: Type::ModuleRef(module),
            sym: module,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(recv),
                name: "apply".into(),
            },
            ty: self.st.get(apply).ty.clone(),
            sym: apply,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        Some(Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(fun),
                args: vec![class_arg],
            },
            ty: Type::Class {
                sym: ct_cls,
                args: vec![t.clone()],
            },
            sym: apply,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        })
    }

    /// A class of the reflection API, loaded if the program has not named it.
    ///
    /// The materialiser reaches for `Mirror` and `TypeCreator`, which
    /// `typeOf[Foo]` never mentions, so nothing has entered a symbol for
    /// them; `ensure_class` reads the pickle the way a signature conversion
    /// would.
    pub(crate) fn reflect_class(&mut self, full_name: &str, jvm: &str) -> Option<SymbolId> {
        if let Some(id) = crate::classpath::find_by_jvm(&self.st, jvm) {
            return Some(id);
        }
        self.pickle
            .ensure_class(&mut self.st, &mut self.binary, full_name, false)
    }

    /// nsc's `materializeTypeTag` / `materializeWeakTypeTag`: a `TypeTag[T]`
    /// is *built*, not found (`crate::materialize`, `docs/macros.md` §7.10).
    ///
    /// Returns `None` when this is not a tag request at all, or when there is
    /// no universe to build one in -- both keep the ordinary "no implicit"
    /// report. A tag request whose type scala-rs cannot rebuild is *named*
    /// here and the error is this method's, because "could not find implicit
    /// value of type TypeTag[List[Int]]" points at the wrong thing: no
    /// program was ever going to define that value.
    fn materialize_tag(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        if !self.library_abi {
            return None;
        }
        let (tag, arg) = crate::materialize::tag_request(&self.st, pt)?;
        // The tag has to name a universe, and which universe it is comes from
        // `import <universe>._` -- the same reading a quasiquote uses.
        let universe = self.universe_in_scope()?;
        let classes = crate::materialize::TagClasses {
            // `TypeTags$TypeTag.class` has no pickle of its own -- a trait's
            // nested class is pickled inside the trait -- so only the
            // enclosing trait's signature can name it.
            tag_cls: self.reflect_class(tag.pickle_name(), tag.jvm())?,
            type_tags: self
                .reflect_class("scala.reflect.api.TypeTags", "scala/reflect/api/TypeTags")?,
            mirror: self.reflect_class("scala.reflect.api.Mirror", "scala/reflect/api/Mirror")?,
            creator: self.reflect_class(
                "scala.reflect.api.TypeCreator",
                "scala/reflect/api/TypeCreator",
            )?,
        };
        let type_api = self.reflect_class(
            "scala.reflect.api.Types.TypeApi",
            "scala/reflect/api/Types$TypeApi",
        )?;
        let (tag_cls, mirror) = (classes.tag_cls, classes.mirror);
        crate::materialize::ensure_tag_module(&mut self.st, tag, classes)?;
        let tag_name = tag.simple().to_string();
        // `TypeTags` is not a *direct* parent of `JavaUniverse` -- it is one
        // of `scala.reflect.api.Universe`'s, and those live only in the
        // pickle. Until something asks the universe for a member it does not
        // have, the link is not there and the accessor above is unreachable:
        // the first `typeOf[T]` in a run reported "value TypeTag is not a
        // member of JavaUniverse" and every later one worked, because the
        // failed lookup itself attached the ancestors. Ask here instead.
        let universe_ty = universe.ty.clone();
        let _ = self.supply_from_pickle(&universe_ty, &tag_name);
        let body = match self.tag_body(tag, &arg, span) {
            Ok(b) => b,
            Err(why) => {
                self.error(
                    span,
                    format!(
                        "materialisation is not implemented: cannot build a \
                         {tag_name} for {why}. scala-rs rebuilds a tag out of \
                         `staticClass` calls, `appliedType` and the tags in \
                         scope; see docs/macros.md \u{a7}7.10 and \u{a7}7.12.",
                    ),
                );
                return Some(Tree {
                    id: NodeId(0),
                    span,
                    kind: TreeKind::Empty,
                    ty: Type::Error,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                });
            }
        };
        self.gensym += 1;
        let creator_name = format!("$typecreator{}", self.gensym);
        let want = Type::Class {
            sym: tag_cls,
            args: vec![arg.clone()],
        };
        let mut tree = crate::materialize::Materialiser {
            universe: &universe,
            creator_name,
            arg,
            body,
            tag_name,
            mirror_ty: Type::Class {
                sym: mirror,
                args: vec![],
            },
            type_api: Type::Class {
                sym: type_api,
                args: vec![],
            },
            span,
        }
        .build();
        self.type_expr(&mut tree, &want);
        Some(tree)
    }

    /// How the synthetic `TypeCreator` rebuilds `ty`, or why it cannot.
    ///
    /// Three shapes, and nothing else (`crate::materialize::TagBody`):
    /// a monomorphic class is one `staticClass` call; a type constructor at
    /// arguments is `appliedType` over the same, applied to each argument's
    /// own body; a type *parameter* is only knowable through a tag in scope,
    /// which is how `c.Expr[F[E]]` works inside `def impl[E](c: Context)
    /// (implicit e: c.WeakTypeTag[E])` -- slick's `TableQueryMacroImpl`.
    ///
    /// The tag for a type parameter is looked up by the ordinary implicit
    /// search, which cannot come back here: materialisation is only the
    /// *fallback* after a search failed, so there is no cycle.
    pub(crate) fn tag_body(
        &mut self,
        tag: crate::materialize::Tag,
        ty: &Type,
        span: Span,
    ) -> Result<crate::materialize::TagBody, String> {
        use crate::materialize::TagBody;
        let flat = self.st.dealias(ty);
        if let Ok(name) = crate::materialize::static_class_name(&self.st, &flat) {
            return Ok(TagBody::StaticClass(name));
        }
        match &flat {
            Type::Class { sym, args } if !args.is_empty() => {
                let class_name = crate::materialize::static_class_of_sym(&self.st, *sym)?;
                let mut built = Vec::new();
                for a in args {
                    built.push(self.tag_body(tag, a, span)?);
                }
                Ok(TagBody::Applied {
                    class_name,
                    args: built,
                })
            }
            // A function, a tuple and an array are `Type`s of their own here
            // and `TypeRef`s to an ordinary class there. nsc's tag for
            // `Tag => E` is `TypeRef(scala.type, Function1, List(Tag, E))`,
            // which is what `appliedType(staticClass("scala.Function1"), …)`
            // builds -- slick's `TableQueryMacroImpl` asks for exactly that
            // one (`c.Expr[Tag => E]`). The arity bound is the library's:
            // `FunctionN` and `TupleN` stop at 22.
            Type::Function { params, ret } if params.len() <= 22 => {
                let mut built = Vec::new();
                for a in params.iter().chain(std::iter::once(&**ret)) {
                    built.push(self.tag_body(tag, a, span)?);
                }
                Ok(TagBody::Applied {
                    class_name: format!("scala.Function{}", params.len()),
                    args: built,
                })
            }
            Type::Tuple(xs) if (1..=22).contains(&xs.len()) => {
                let mut built = Vec::new();
                for a in xs {
                    built.push(self.tag_body(tag, a, span)?);
                }
                Ok(TagBody::Applied {
                    class_name: format!("scala.Tuple{}", xs.len()),
                    args: built,
                })
            }
            Type::Array(elem) => Ok(TagBody::Applied {
                class_name: "scala.Array".to_string(),
                args: vec![self.tag_body(tag, elem, span)?],
            }),
            Type::TypeParam(id) | Type::TypeMember(id) => {
                let name = self.st.get(*id).name.clone();
                let Some(tag_cls) = self.reflect_class(tag.pickle_name(), tag.jvm()) else {
                    return Err(format!("`{name}`, an abstract type"));
                };
                let want = Type::Class {
                    sym: tag_cls,
                    args: vec![flat.clone()],
                };
                self.warm_implicit_scope(&want);
                match self.search_implicit(&want) {
                    crate::implicits::ImplicitSearch::Found(id) => Ok(TagBody::FromTag(Box::new(
                        self.implicit_tree(id, &want, span, 0),
                    ))),
                    // Same wording as `static_class_name`'s: this is the
                    // same refusal, reached after the implicit search that
                    // could have supplied the tag came up empty.
                    _ => Err(format!("`{name}`, an abstract type with no tag in scope")),
                }
            }
            other => Err(crate::materialize::static_class_name(&self.st, other)
                .err()
                .unwrap_or_else(|| format!("`{}`", self.st.display_type(other)))),
        }
    }

    /// nsc: `A <: B` is a view `A => B` (identity / asInstanceOf).
    fn identity_view(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        let Type::Function { params, ret } = pt else {
            return None;
        };
        if params.len() != 1 {
            return None;
        }
        if !self.st.is_sub_type(&params[0], ret) {
            return None;
        }
        let from = params[0].clone();
        let to = (**ret).clone();
        self.gensym += 1;
        let pname = format!("x${}", self.gensym);
        let pid = self.st.alloc(
            &pname,
            self.st.owner,
            crate::symbol::SymKind::Term,
            Flags::PARAM.with(Flags::SYNTHETIC),
            "",
        );
        self.st.get_mut(pid).ty = from.clone();
        let ident = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: pname.clone(),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let param = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name: pname,
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let mut lam = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Function {
                vparams: vec![param],
                body: Box::new(ident),
            },
            ty: Type::Function {
                params: vec![from],
                ret: Box::new(to.clone()),
            },
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        self.type_expr(&mut lam, pt);
        self.adapt(&mut lam, pt);
        Some(lam)
    }

    /// Prefer 4-arg `flatMap[BS, B]` when the lambda returns `Array`, else 3-arg.
    pub(crate) fn bind_array_ops_flat_map(
        &mut self,
        fun: &mut Tree,
        args: &mut [Tree],
        recv_ty: Option<&Type>,
        arg_tys: &mut [Type],
    ) {
        let Some(a0) = args.first_mut() else {
            return;
        };
        if matches!(a0.kind, TreeKind::Function { .. }) && a0.ty.is_no_type() {
            let elem = recv_ty.and_then(|t| self.elem_type(t)).unwrap_or(Type::Any);
            let pt = Type::Function {
                params: vec![elem.clone()],
                ret: Box::new(Type::Any),
            };
            self.type_expr(a0, &pt);
            if let TreeKind::Function { body, .. } = &a0.kind {
                let body_ty = body.ty.widen_constant();
                if !body_ty.is_no_type() && !body_ty.is_error() {
                    a0.ty = Type::Function {
                        params: vec![elem],
                        ret: Box::new(body_ty),
                    };
                }
            }
            if let Some(slot) = arg_tys.first_mut() {
                *slot = a0.ty.clone();
            }
        }
        let lambda_ret = match arg_tys.first() {
            Some(Type::Function { ret, .. }) => ret.as_ref(),
            _ => return,
        };
        let want_four = matches!(lambda_ret, Type::Array(_));
        let Some(owner) = recv_ty.and_then(|t| self.st.class_sym_of(t)) else {
            return;
        };
        let methods = self.st.lookup_member(owner, "flatMap");
        let Some(picked) = methods.into_iter().find(|m| {
            let n = self.st.get(*m).tparams.len();
            if want_four {
                n >= 2
            } else {
                n == 1
            }
        }) else {
            return;
        };
        fun.sym = picked;
        let mut ty = self.st.get(picked).ty.clone();
        if let Some(Type::Class { args, .. }) = recv_ty {
            if !args.is_empty() {
                ty = self.st.subst_tparams(owner, args, &ty);
            }
        }
        fun.ty = ty;
    }

    /// nsc `implicit asIterable: Array[Int] => Iterable[Int]` is `Predef.wrapIntArray`.
    fn array_wrap_view(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        let Type::Function { params, ret } = pt else {
            return None;
        };
        if params.len() != 1 {
            return None;
        }
        let Type::Array(elem) = &params[0] else {
            return None;
        };
        if !matches!(elem.as_ref(), Type::Int) {
            return None;
        }
        let wrap = {
            let from_scope = self.st.lookup("wrapIntArray");
            if let Some(id) = from_scope.into_iter().next() {
                id
            } else {
                let cls = match &self.st.get(self.st.predef).ty {
                    Type::ModuleRef(id) => *id,
                    _ => return None,
                };
                self.st
                    .lookup_member(cls, "wrapIntArray")
                    .into_iter()
                    .next()?
            }
        };
        let from = params[0].clone();
        let to = (**ret).clone();
        self.gensym += 1;
        let pname = format!("x${}", self.gensym);
        let pid = self.st.alloc(
            &pname,
            self.st.owner,
            crate::symbol::SymKind::Term,
            Flags::PARAM.with(Flags::SYNTHETIC),
            "",
        );
        self.st.get_mut(pid).ty = from.clone();
        let ident = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: pname.clone(),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let param = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name: pname,
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let wrap_fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "wrapIntArray".into(),
            },
            ty: self.st.get(wrap).ty.clone(),
            sym: wrap,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let body = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(wrap_fun),
                args: vec![ident],
            },
            ty: to.clone(),
            sym: wrap,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let mut lam = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Function {
                vparams: vec![param],
                body: Box::new(body),
            },
            ty: Type::Function {
                params: vec![from],
                ret: Box::new(to.clone()),
            },
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        self.type_expr(&mut lam, pt);
        self.adapt(&mut lam, pt);
        Some(lam)
    }
}
