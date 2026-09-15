//! Answering the engine's questions: reverse RPC from the JVM back into the
//! typer (`docs/macros.md` §7.18, step 1).
//!
//! Expansion used to be one line out and one line back. It is now a
//! conversation. An implementation running inside the engine can stop and ask
//! scala-rs something only scala-rs knows -- above all *what does this tree
//! mean here* -- and scala-rs answers by really doing the work, in the real
//! typer, in the scope the macro was called from.
//!
//! ```text
//! scala-rs (Rust)                        engine (JVM)
//! ───────────────                        ────────────
//! (expand …)               ──────→       invoke the implementation
//!                                          c.typecheck(q"…")
//!                          ←──────       (q typecheck …)
//! type it, here, now
//!                          ──────→       (a ok …)   ← a Tree with a type
//!                                        … the implementation goes on
//!                          ←──────       (ok <expansion>)
//! ```
//!
//! **Why this cannot be a snapshot.** The alternative -- send the engine a
//! description of the run's symbols up front -- is the thing `docs/macros.md`
//! §5.1 shows to be impossible: while `lazy val Issues = TableQuery[Issues]`
//! is being typed, the members of `class Issues` are still un-inferred, so
//! there is no moment at which a correct snapshot could be taken. Asking
//! instead forces exactly the lazy signature the typer would have forced, at
//! the moment the question is asked.
//!
//! **Nothing here guesses.** Every question has three possible answers: the
//! real one, "the typer rejected that" (which the engine raises as
//! `TypecheckException`, the way nsc does), and `(no "reason")` -- scala-rs
//! cannot answer *this* question -- which becomes a compile diagnostic naming
//! what was missing. A question scala-rs would have to guess at is the third.

use scala_rs_parser::{Flags, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_pickle::names::encode_method_name;
use scala_rs_span::Level;

use crate::check::Typer;
use crate::expand::{at, lit_to_wire, quote_into, scala_full_name, Sexp, WireCx};
use crate::symbol::{SymKind, SymbolTable};

/// The question's kind as a fixed name, for the timing table
/// (`SCALA_RS_MACRO_TIMING=1`). A kind this module does not answer is still
/// counted, under `<other>`, so the table adds up.
pub(crate) fn query_kind(items: &[Sexp]) -> &'static str {
    let Ok(kind) = at(items, 1) else {
        return "<malformed>";
    };
    match kind.text().as_str() {
        "typecheck" => "typecheck",
        "inferImplicitValue" => "inferImplicitValue",
        "enclosingOwner" => "enclosingOwner",
        "functionSymbol" => "functionSymbol",
        "symbol" => "symbol",
        "symbolInfo" => "symbolInfo",
        "companion" => "companion",
        "modulePair" => "modulePair",
        "viewInfo" => "viewInfo",
        _ => "<other>",
    }
}

impl Typer {
    pub(crate) fn macro_current_owner(&self) -> SymbolId {
        if self.macro_lexical_owner.is_none() {
            self.st.owner
        } else {
            self.macro_lexical_owner
        }
    }

    pub(crate) fn macro_enter_owner(&mut self, sym: SymbolId) -> SymbolId {
        let saved = self.macro_lexical_owner;
        if !sym.is_none() {
            let physical = self.st.get(sym).owner;
            let parent = if !saved.is_none() && physical == self.st.owner && saved != sym {
                saved
            } else {
                physical
            };
            self.macro_mirror_owners.entry(sym).or_insert(parent);
            self.macro_lexical_owner = sym;
        }
        saved
    }

    pub(crate) fn macro_function_symbol(&mut self, node: scala_rs_parser::NodeId) -> SymbolId {
        if let Some(&sym) = self.macro_function_symbols.get(&(self.file_index, node)) {
            return sym;
        }
        let owner = self.macro_current_owner();
        let sym = self
            .st
            .alloc("$anonfun", owner, SymKind::Method, Flags::SYNTHETIC, "");
        self.macro_function_symbols
            .insert((self.file_index, node), sym);
        self.macro_mirror_owners.insert(sym, owner);
        sym
    }

    fn answer_mirror_symbol(&mut self, items: &[Sexp]) -> String {
        let Ok(id) = at(items, 2).and_then(|x| x.text().parse::<u32>().map_err(|e| e.to_string()))
        else {
            return refusal("invalid mirror symbol identity");
        };
        let kind = items[1].text();
        if kind == "functionSymbol" {
            return match self
                .macro_function_symbols
                .get(&(self.file_index, scala_rs_parser::NodeId(id)))
            {
                Some(sym) => format!("(a ref {})", sym.0),
                // A typed macro result can contain a function nested in a
                // generated class body.  Its enclosing class is rebuilt only
                // when a later macro reads that class as `c.prefix`, so the
                // ordinary function-literal typer has not visited the body
                // yet.  Allocate the same per-run synthetic symbol lazily;
                // this preserves the lexical owner without cloning the
                // subtree or keeping engine-global state.
                None => {
                    let sym = self.macro_function_symbol(NodeId(id));
                    format!("(a ref {})", sym.0)
                }
            };
        }
        let sym = SymbolId(id);
        if sym.is_none() || id as usize >= self.st.symbols.len() {
            return refusal("unknown mirror symbol identity");
        }
        if kind == "companion" {
            return format!("(a ref {})", self.mirror_companion(sym).0);
        }
        if kind == "modulePair" {
            return match self.mirror_module_pair(sym) {
                Some((m, c)) => format!("(a pair {} {})", m.0, c.0),
                None => refusal(&format!("`{}` is not an object", self.st.get(sym).name)),
            };
        }
        if kind == "symbol" {
            let s = self.st.get(sym);
            let owner = self
                .macro_mirror_owners
                .get(&sym)
                .copied()
                .unwrap_or(s.owner);
            // A trait is `abstract` in nsc as well, and `<interface>` when
            // every member of it is abstract.
            let mut class_flags = s.flags;
            if s.flags.contains(Flags::TRAIT) {
                class_flags = class_flags.with(Flags::ABSTRACT);
                if self.is_pure_interface(sym) {
                    class_flags = class_flags.with(Flags::INTERFACE);
                }
            }
            // An object's name is `Foo$` here and `Foo` in nsc; so is the
            // last segment of its full name.
            let mut full = scala_full_name(&self.st, sym);
            if matches!(s.kind, SymKind::Module | SymKind::ModuleClass) {
                full = full.trim_end_matches('.').to_string();
            }
            return format!(
                "(a symbol {} {} {} {} {}{})",
                id,
                quoted(&format!("{:?}", s.kind)),
                quoted(&s.name),
                quoted(&full),
                owner.0,
                if s.is_class_like() {
                    class_flags_wire(class_flags)
                } else {
                    member_flags_wire(s.flags, s.kind, s.name == "<init>")
                }
            );
        }
        match self.mirror_symbol_info(sym) {
            Ok(info) => format!("(a info {info})"),
            Err(why) => refusal(&why),
        }
    }

    fn mirror_symbol_info(&mut self, sym: SymbolId) -> Result<String, String> {
        // nsc's typed anonymous-function symbol intentionally has NoType.
        if self.macro_function_symbols.values().any(|&id| id == sym) {
            return Ok("(notype)".to_string());
        }
        let s = self.st.get(sym).clone();
        if s.kind == SymKind::TypeParam {
            let lo = self.type_to_wire(s.bound_lo.as_ref().unwrap_or(&Type::Nothing))?;
            let hi = self.type_to_wire(s.bound_hi.as_ref().unwrap_or(&Type::Any))?;
            let bounds = format!("(bounds {lo} {hi})");
            return Ok(if s.tparams.is_empty() { bounds } else {
                format!("(poly (params {}) {bounds})", s.tparams.iter().map(|p| p.0.to_string()).collect::<Vec<_>>().join(" "))
            });
        }
        if s.is_class_like() {
            return self.mirror_class_info(sym);
        }
        if s.kind == SymKind::TypeMember {
            let info = if s.is_type_alias {
                self.type_to_wire(&s.ty)?
            } else {
                format!("(bounds {} {})", self.type_to_wire(s.bound_lo.as_ref().unwrap_or(&Type::Nothing))?, self.type_to_wire(s.bound_hi.as_ref().unwrap_or(&Type::Any))?)
            };
            return Ok(if s.tparams.is_empty() { info } else {
                format!("(poly (params {}) {info})", s.tparams.iter().map(|p| p.0.to_string()).collect::<Vec<_>>().join(" "))
            });
        }
        let polymorphic = |info: String| {
            if s.tparams.is_empty() { info } else {
                format!("(poly (params {}) {info})", s.tparams.iter().map(|p| p.0.to_string()).collect::<Vec<_>>().join(" "))
            }
        };
        let ty = s.ty;
        if ty.is_no_type() {
            return Err(format!("recursive value {} needs type", s.name));
        }
        if let Type::Method { paramss, ret } = ty {
            // A constructor returns `Unit` in scala-rs's model and the class
            // in nsc's.
            let ret = if s.name == "<init>" && self.st.get(s.owner).is_class_like() {
                Type::Class {
                    sym: s.owner,
                    args: self
                        .st
                        .get(s.owner)
                        .tparams
                        .iter()
                        .copied()
                        .map(Type::TypeParam)
                        .collect(),
                }
            } else {
                *ret
            };
            if ret.is_no_type() {
                return Err(format!("recursive method {} needs result type", s.name));
            }
            let mut info = self.type_to_wire(&ret)?;
            // nsc gives a default getter's result `@uncheckedVariance`, so that
            // a default may mention a variant type parameter
            // (`copy$default$1: Int @scala.annotation.unchecked.uncheckedVariance`).
            if s.name.contains("$default$") && self.st.get(s.owner).is_class_like() {
                info = format!("(annot \"scala.annotation.unchecked.uncheckedVariance\" {info})");
            }
            if paramss.is_empty() {
                return Ok(polymorphic(format!("(nullary {info})")));
            }
            if s.params.len() != paramss.iter().map(Vec::len).sum::<usize>() {
                return Err(format!(
                    "mirror parameter identities are incomplete for {}",
                    s.name
                ));
            }
            let mut end = s.params.len();
            for params in paramss.iter().rev() {
                let start = end - params.len();
                let mut args = Vec::new();
                for (&param, ty) in s.params[start..end].iter().zip(params) {
                    let ty = if ty.is_no_type() {
                        self.st.get(param).ty.clone()
                    } else {
                        ty.clone()
                    };
                    let wired = self.type_to_wire(&ty)?;
                    if self.st.get(param).owner != sym {
                        // A parameter whose symbol belongs to something else:
                        // a case class's constructor shares its fields'. It
                        // travels by name and flags, as a parameter of its
                        // own on the far side, rather than as the field.
                        let mut named = String::from("(argn ");
                        quote_into(&mut named, &self.st.get(param).name);
                        named.push_str(if self.st.get(param).flags.contains(Flags::DEFAULTPARAM) {
                            " (f \"PARAM\" \"DEFAULTPARAM\") "
                        } else {
                            " (f \"PARAM\") "
                        });
                        named.push_str(&wired);
                        named.push(')');
                        args.push(named);
                        continue;
                    }
                    args.push(format!("(arg {} {})", param.0, wired));
                }
                info = format!("(method (params {}) {info})", args.join(" "));
                end = start;
            }
            return Ok(polymorphic(info));
        }
        self.type_to_wire(&ty)
    }

    /// `(q viewInfo <getter|setter|field> <id>)`: one of the symbols nsc
    /// makes of a scala-rs `val` ([`Typer::mirror_view_info`]).
    fn answer_view_info(&mut self, items: &[Sexp]) -> String {
        let (Ok(view), Ok(id)) = (
            at(items, 2).map(|v| v.text()),
            at(items, 3).and_then(|x| x.text().parse::<u32>().map_err(|e| e.to_string())),
        ) else {
            return refusal("the macro engine asked a malformed `viewInfo`");
        };
        if id == 0 || id as usize >= self.st.symbols.len() {
            return refusal("unknown mirror symbol identity");
        }
        match self.mirror_view_info(&view, SymbolId(id)) {
            Ok(info) => format!("(a info {info})"),
            Err(why) => refusal(&why),
        }
    }

    /// Answer one `(q …)` the engine wrote, as the line to write back.
    ///
    /// Never `Err`: a question that cannot be answered is answered with
    /// `(no "reason")`, which the engine turns into an exception naming the
    /// reason, which becomes the call site's diagnostic. Returning an error
    /// here instead would leave the engine blocked on a read.
    pub(crate) fn answer_query(&mut self, items: &[Sexp]) -> String {
        let kind = match at(items, 1) {
            Ok(k) => k.text(),
            Err(_) => return refusal("the macro engine asked a malformed question"),
        };
        match kind.as_str() {
            "typecheck" => self.answer_typecheck(items),
            "inferImplicitValue" => self.answer_infer_implicit_value(items),
            "enclosingOwner" => format!("(a ref {})", self.macro_current_owner().0),
            "functionSymbol" | "symbol" | "symbolInfo" | "companion" | "modulePair" => {
                self.answer_mirror_symbol(items)
            }
            "viewInfo" => self.answer_view_info(items),
            other => refusal(&format!(
                "the macro engine asked scala-rs `{other}`, which it does not answer"
            )),
        }
    }

    /// `c.inferImplicitValue(pt, silent, withMacrosDisabled, pos)`.
    ///
    /// The engine can inspect and construct reflect types, but the implicit
    /// scope belongs to this typer.  Bring the requested type back, run the
    /// same search and tree construction used for an omitted implicit
    /// argument, and return the fully typed witness.  A miss is `EmptyTree`,
    /// as in nsc.  When `silent` is false its diagnostic remains attached to
    /// the macro call site; speculative diagnostics are otherwise rolled
    /// back.
    fn answer_infer_implicit_value(&mut self, items: &[Sexp]) -> String {
        if items.len() != 5 {
            return refusal("the macro engine asked a malformed `c.inferImplicitValue`");
        }
        let Ok(wanted) = at(items, 2) else {
            return refusal("the macro engine asked a malformed `c.inferImplicitValue`");
        };
        let Some(silent) = wire_bool(items.get(3)) else {
            return refusal(
                "the macro engine asked a malformed `c.inferImplicitValue` `silent` flag",
            );
        };
        let Some(no_macros) = wire_bool(items.get(4)) else {
            return refusal(
                "the macro engine asked a malformed `c.inferImplicitValue` `withMacrosDisabled` flag",
            );
        };
        if self.macro_query_depth >= MAX_QUERY_DEPTH {
            return refusal(&format!(
                "`c.inferImplicitValue` was asked more than {MAX_QUERY_DEPTH} deep; \
                 scala-rs stops rather than recurse further"
            ));
        }
        self.with_macro_query_depth(|this| {
            this.answer_infer_implicit_value_in(wanted, silent, no_macros)
        })
    }

    fn answer_infer_implicit_value_in(
        &mut self,
        wanted: &Sexp,
        silent: bool,
        no_macros: bool,
    ) -> String {
        let span = self.macro_rpc_span;
        let mark = self.diags.len();
        let resolved = self.query_type_from_wire(wanted, span);
        // Resolution may have emitted diagnostics before discovering that
        // the requested wire is unsupported. Every exit from the query is
        // speculative until `silent = false` deliberately reports a miss.
        let resolution_error = self.take_probe_errors(mark);
        let pt = match resolved {
            Ok(t) => t,
            Err(why) => {
                let detail = match resolution_error {
                    Some(msg) => format!("{why}: {msg}"),
                    None => why,
                };
                return refusal(&format!("`c.inferImplicitValue` asked for {detail}"));
            }
        };
        if let Some(msg) = resolution_error {
            return refusal(&format!(
                "`c.inferImplicitValue` asked for a type scala-rs could not resolve: {msg}"
            ));
        }
        if pt.is_error() || pt.is_no_type() || matches!(pt, Type::Named { .. }) {
            return refusal(
                "`c.inferImplicitValue` asked for a type scala-rs could not resolve at the macro call site",
            );
        }

        self.warm_implicit_scope(&pt);
        self.with_implicit_macros_disabled(no_macros, |this| {
            this.answer_infer_implicit_search(&pt, silent, mark)
        })
    }

    fn answer_infer_implicit_search(&mut self, pt: &Type, silent: bool, mark: usize) -> String {
        let span = self.macro_rpc_span;
        let mut search = self.search_implicit(pt);
        if matches!(search, crate::implicits::ImplicitSearch::None)
            && self.warm_implicit_candidates(std::slice::from_ref(pt))
        {
            search = self.search_implicit(pt);
        }
        match search {
            crate::implicits::ImplicitSearch::Found(id) => {
                // `implicit_tree` expands a selected implicit macro. During a
                // reverse query the engine is already busy with the outer
                // macro, so a normal macro records a failure and remains an
                // unexpanded reference. Never serialize that reference as a
                // successful implicit answer.
                let key = self.macro_failure_key(span);
                let (mut tree, nested_failure) = self
                    .with_isolated_macro_failure(key, |this| this.implicit_tree(id, pt, span, 0));
                let selected_unexpanded_macro =
                    self.st.get(id).macro_impl.is_some() && tree.sym == id;
                self.adapt(&mut tree, pt);
                let failures = self.take_probe_errors(mark);
                if let Some(why) = nested_failure {
                    return refusal(&format!(
                        "`c.inferImplicitValue` selected implicit macro `{}`, but scala-rs could not expand it while answering the outer macro: {why}",
                        self.st.get(id).name
                    ));
                }
                if selected_unexpanded_macro {
                    return refusal(&format!(
                        "`c.inferImplicitValue` selected implicit macro `{}`, but its expansion did not complete",
                        self.st.get(id).name
                    ));
                }
                if tree.ty.is_error() || failures.is_some() {
                    if !silent {
                        self.error(
                            span,
                            failures.unwrap_or_else(|| self.missing_implicit_message(pt, None)),
                        );
                    }
                    return "(a none)".to_string();
                }

                // An abstract/path-dependent result equal to the target is
                // already present as the JVM `pt` object. Sending a class
                // name for it would name a different type.
                let ty = if tree.ty == *pt {
                    "(same)".to_string()
                } else {
                    match self.type_to_wire(&tree.ty) {
                        Ok(ty) => ty,
                        Err(why) => {
                            return refusal(&format!(
                                "`c.inferImplicitValue` found an implicit of {why}, which scala-rs cannot describe to the macro engine"
                            ));
                        }
                    }
                };
                let mut built = String::new();
                let types = crate::expand::WireTypes::default();
                let cx = crate::expand::WireCx {
                    st: &self.st,
                    types: &types,
                };
                match answer_tree_to_wire(&cx, &tree, &mut built) {
                    Ok(()) => format!("(a ok {ty} {built})"),
                    Err(why) => refusal(&format!("`c.inferImplicitValue` produced {why}")),
                }
            }
            crate::implicits::ImplicitSearch::None => {
                self.diags.truncate(mark);
                if !silent {
                    self.error(span, self.missing_implicit_message(pt, None));
                }
                "(a none)".to_string()
            }
            crate::implicits::ImplicitSearch::Ambiguous(ids) => {
                self.diags.truncate(mark);
                if !silent {
                    self.error(
                        span,
                        format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                    );
                }
                "(a none)".to_string()
            }
        }
    }

    /// Read a type the macro universe asked the call-site typer about.
    ///
    /// Ordinary class types reuse the expansion wire. `(mem owner name ...)`
    /// is a path-dependent or abstract member: the JVM sends the member's
    /// static declaration owner and identity, and this side installs that
    /// exact pickled symbol. Sending its printed prefix (`x.T`) would require
    /// the implementation's `x` to be in the call site's lexical scope,
    /// which it is not -- ZIO's `Tracer.instance.Type` is the concrete case.
    pub(crate) fn query_type_from_wire(
        &mut self,
        wire: &Sexp,
        span: scala_rs_span::Span,
    ) -> Result<Type, String> {
        let items = wire.list()?;
        if items.first().and_then(|s| s.atom()) != Some("mem") {
            let tpt = self.type_tree_from_wire(wire, span)?;
            return Ok(self.tree_to_type(&tpt));
        }
        let owner_name = at(items, 1)?.text();
        let member_name = at(items, 2)?.text();
        let member = self.macro_type_member(&owner_name, &member_name, span)?;
        let mut base = if self.st.get(member).kind == SymKind::TypeParam { Type::TypeParam(member) } else { Type::TypeMember(member) };
        let mut arg_start = 3;
        if let Some(prefix) = items.get(3).and_then(|p| p.list().ok()) {
            if prefix.first().and_then(|p| p.atom()) == Some("pre") {
                let term_owner = at(prefix, 1)?.text();
                let term_name = at(prefix, 2)?.text();
                let prefix_tree =
                    crate::expand::path_tree(&format!("{term_owner}.{term_name}"), span);
                base = self
                    .macro_path_dependent_type(&prefix_tree, &member_name, span)
                    .ok_or_else(|| {
                        format!("stable prefix `{term_owner}.{term_name}` does not resolve")
                    })?;
                arg_start = 4;
            }
        }
        let args = items.get(arg_start..).unwrap_or(&[]);
        if args.is_empty() {
            return Ok(base);
        }
        let args = args
            .iter()
            .map(|arg| self.query_type_from_wire(arg, span))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Type::Applied {
            ctor: Box::new(base),
            args,
        })
    }

    /// Resolve the declaration identity the macro engine uses for a
    /// path-dependent type.  Both query types and returned tree symbols use
    /// this path, so an abstract member round-trips as the same pickled
    /// symbol rather than a nominal approximation.
    pub(crate) fn macro_type_member(
        &mut self,
        owner_name: &str,
        member_name: &str,
        span: scala_rs_span::Span,
    ) -> Result<scala_rs_parser::SymbolId, String> {
        let owner_tree = crate::expand::path_tree(owner_name, span);
        let owner_ty = self.tree_to_type(&owner_tree);
        let owner = self.st.class_sym_of(&owner_ty).ok_or_else(|| {
            format!("the member owner `{owner_name}` does not resolve to a class")
        })?;
        if let Some(parameter) = self.st.get(owner).tparams.iter().copied().find(|p| self.st.get(*p).name == member_name) {
            return Ok(parameter);
        }
        self.complete_binary_member(owner, member_name, span);
        self.st
            .lookup_member(owner, member_name)
            .into_iter()
            .find(|id| self.st.get(*id).kind == SymKind::TypeMember)
            .ok_or_else(|| format!("type member `{owner_name}.{member_name}` does not resolve"))
    }

    /// `c.typecheck(tree, mode, pt, silent, …)`.
    ///
    /// nsc typechecks in the macro call site's own context. So does this: the
    /// typer is, at this instant, in the middle of typing the application that
    /// started the expansion, so its scope *is* the call site's scope and
    /// every name the call site can see resolves. That is what makes the run's
    /// own classes -- the ones with no class file for the engine's mirror to
    /// find -- answerable at all.
    ///
    /// Diagnostics the attempt produces are rolled back rather than reported.
    /// A failed `c.typecheck` is not a compile error: nsc raises
    /// `TypecheckException` and lets the implementation decide, and an
    /// implementation that catches it (`macro-typecheck-implicitsdisabled`
    /// does, and so does slick's own `TableQuery` probing) must not leave an
    /// error behind on the way.
    fn answer_typecheck(&mut self, items: &[Sexp]) -> String {
        if items.len() != 5 {
            return refusal("the macro engine asked a malformed `c.typecheck`");
        }
        let (Ok(tree_sexp), Ok(mode_sexp)) = (at(items, 2), at(items, 3)) else {
            // Unreachable through the engine, which always writes all three;
            // said rather than unwrapped, because a malformed line must not
            // leave the engine blocked on a read.
            return refusal("the macro engine asked a malformed `c.typecheck`");
        };
        if matches!(mode_sexp, Sexp::List(_)) || wire_bool(items.get(4)).is_none() {
            return refusal("the macro engine asked a malformed `c.typecheck`");
        }
        let mode = mode_sexp.text();
        // `c.typecheck` is re-entrant in nsc. Here each nested question would
        // need its own conversation on a pipe that carries one, and the
        // `converse` loop is what serialises them -- but a *tree* that itself
        // triggers another query cannot be, so the depth is bounded and named.
        if self.macro_query_depth >= MAX_QUERY_DEPTH {
            return refusal(&format!(
                "`c.typecheck` was asked more than {MAX_QUERY_DEPTH} deep; \
                 scala-rs stops rather than recurse further"
            ));
        }
        let span = self.macro_rpc_span;
        // Tree reconstruction is part of the speculative transaction too:
        // resolving a returned path-dependent symbol can complete a binary
        // member and emit diagnostics before ultimately refusing the wire.
        let mark = self.diags.len();
        let rebuilt = self.tree_from_reply(tree_sexp, span);
        let reconstruction_errors = self.take_probe_errors(mark);
        let mut tree = match rebuilt {
            Ok(t) if reconstruction_errors.is_none() => t,
            Ok(_) => {
                return refusal(&format!(
                    "`c.typecheck` could not rebuild its tree: {}",
                    reconstruction_errors.unwrap()
                ))
            }
            Err(why) => {
                let detail = reconstruction_errors
                    .map(|diagnostic| format!("{why}: {diagnostic}"))
                    .unwrap_or(why);
                return refusal(&detail);
            }
        };
        // A macro application inside the tree is refused (`macro_engine_busy`),
        // and the refusal is recorded against the span every node of a rebuilt
        // tree carries -- which is the *outer* call site's. Left there it would
        // be a reason attached to a call that succeeded, so it is put back the
        // way it was, exactly like the diagnostics.
        let key = self.macro_failure_key(span);
        let (answer, _nested_failure) = self.with_isolated_macro_failure(key, |this| {
            this.with_macro_query_depth(|this| match mode.as_str() {
                "TERM" => this.typecheck_term(&mut tree),
                "TYPE" => this.typecheck_type(&tree),
                other => refusal(&format!(
                    "`c.typecheck` was asked for {other}mode, which scala-rs does \
                     not implement (only TERMmode and TYPEmode)"
                )),
            })
        });
        answer
    }

    /// TERMmode: type the tree as an expression and hand back what the typer
    /// made of it.
    fn typecheck_term(&mut self, tree: &mut Tree) -> String {
        let mark = self.diags.len();
        self.type_expr(tree, &Type::NoType);
        let failures = self.take_probe_errors(mark);
        if let Some(msg) = failures {
            return format!("(a fail {})", quoted(&msg));
        }
        if tree.ty.is_error() || tree.ty.is_no_type() {
            return format!(
                "(a fail {})",
                quoted("the tree does not typecheck at the macro call site")
            );
        }
        // An unresolved overload is not a value. nsc rejects this TERM query
        // (and its silent caller receives EmptyTree), before serializing a type.
        if matches!(tree.ty, Type::Overload(_)) {
            return format!(
                "(a fail {})",
                quoted("missing argument list for overloaded method")
            );
        }
        let typed = tree.ty.clone();
        let ty = match self.type_to_wire(&typed) {
            Ok(t) => t,
            Err(why) => {
                return refusal(&format!(
                    "`c.typecheck` typed the tree as {why}, which scala-rs \
                     cannot describe to the macro engine"
                ))
            }
        };
        let mut built = String::new();
        let mut types = crate::expand::WireTypes::default();
        self.collect_wire_types(tree, &mut types);
        let cx = crate::expand::WireCx {
            st: &self.st,
            types: &types,
        };
        match answer_tree_to_wire(&cx, tree, &mut built) {
            Ok(()) => format!("(a ok {ty} {built})"),
            Err(why) => refusal(&format!("`c.typecheck` produced {why}")),
        }
    }

    /// TYPEmode: read the tree as a type and hand back a `TypeTree` carrying
    /// it. nsc answers the same way -- `c.typecheck(tq"List[Int]",
    /// c.TYPEmode).tpe` is `List[Int]` and the tree is a `TypeTree`.
    fn typecheck_type(&mut self, tree: &Tree) -> String {
        let mark = self.diags.len();
        let ty = self.tree_to_type(tree);
        let failures = self.take_probe_errors(mark);
        if let Some(msg) = failures {
            return format!("(a fail {})", quoted(&msg));
        }
        // `resolve_type_name` hands back `Type::Named` for a name that found
        // nothing at all and reported nothing; that is a rejection, not an
        // answer (`crates/typer/src/check.rs`, `unresolved_in`).
        if ty.is_error() || ty.is_no_type() || matches!(ty, Type::Named { .. }) {
            return format!(
                "(a fail {})",
                quoted("not found: the type does not resolve at the macro call site")
            );
        }
        match self.type_to_wire(&ty) {
            Ok(t) => format!("(a ok {t} (t \"TypeTree\" (s0)))"),
            Err(why) => refusal(&format!(
                "`c.typecheck` read the tree as {why}, which scala-rs cannot \
                 describe to the macro engine"
            )),
        }
    }

    /// The errors a speculative typing produced, rolled back off `diags`.
    ///
    /// The message is phrased the way nsc's `TypecheckException` carries one:
    /// the first error, which is the one an implementation prints.
    fn take_probe_errors(&mut self, mark: usize) -> Option<String> {
        let first = self.diags[mark..]
            .iter()
            .find(|d| d.level == Level::Error)
            .map(|d| d.message.clone());
        self.diags.truncate(mark);
        first
    }
}

/// How deep a chain of `c.typecheck` questions may go.
const MAX_QUERY_DEPTH: usize = 16;

fn wire_bool(s: Option<&Sexp>) -> Option<bool> {
    match s?.atom()? {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn refusal(why: &str) -> String {
    format!("(no {})", quoted(why))
}

fn quoted(s: &str) -> String {
    let mut out = String::new();
    quote_into(&mut out, s);
    out
}

impl Typer {
    /// A type the engine can rebuild in the runtime universe, or why it
    /// cannot.
    ///
    /// `(ty "a.b.C" <arg>…)` resolves a classpath class; `(src <id> <arg>…)`
    /// retains a source class or weak type parameter identity. Constant and
    /// structural types use their corresponding reflection representations.
    /// Unsupported shapes are refused rather than widened to another type.
    pub(crate) fn type_to_wire(&mut self, ty: &Type) -> Result<String, String> {
        // An inner class behind a prefix (`prefix.rs`) is the class it views;
        // the engine is handed the class, as for the bare type.
        let ty = crate::prefix::strip_view(ty);
        if let Some(wire) = self.source_type_wire(ty)? {
            return Ok(wire);
        }
        if let Some(wire) = self.binary_type_param_wire(ty)? {
            return Ok(wire);
        }
        if let Some(wire) = self.binary_module_type_wire(ty) {
            return Ok(wire);
        }
        if let Type::Class { sym, args } = ty {
            let jvm = self.st.jvm_internal(*sym);
            if !self.st.is_source_class(*sym)
                && jvm.rsplit('/').next().is_some_and(|name| name.contains('$'))
            {
                let mut wire = format!("(jclass {}", quoted(&jvm.replace('/', ".")));
                for arg in args {
                    wire.push(' ');
                    wire.push_str(&self.type_to_wire(arg)?);
                }
                wire.push(')');
                return Ok(wire);
            }
        }
        let structural = match ty {
            Type::Function { params, ret } if params.len() <= 22 => {
                let mut args = params.clone();
                args.push((**ret).clone());
                Some((format!("scala.Function{}", params.len()), args))
            }
            Type::Tuple(args) if (1..=22).contains(&args.len()) => {
                Some((format!("scala.Tuple{}", args.len()), args.clone()))
            }
            Type::Array(elem) => Some(("scala.Array".to_string(), vec![(**elem).clone()])),
            _ => None,
        };
        if let Some((name, args)) = structural {
            let mut out = format!("(ty {}", quoted(&name));
            for arg in args {
                out.push(' ');
                out.push_str(&self.type_to_wire(&arg)?);
            }
            out.push(')');
            return Ok(out);
        }
        if let Type::Constant(lit) = ty {
            let mut out = String::from("(cst ");
            lit_to_wire(lit, &mut out)?;
            out.push(')');
            return Ok(out);
        }
        if let Type::Class { sym, args } = ty {
            let sym = *sym;
            if !args.is_empty() {
                let name = crate::materialize::static_class_of_sym(&self.st, sym)
                    .map_err(|why| format!("a type whose class is {why}"))?;
                let mut written = Vec::new();
                for a in args {
                    written.push(self.type_to_wire(&a.clone())?);
                }
                let mut out = String::from("(ty ");
                quote_into(&mut out, &name);
                for a in written {
                    out.push(' ');
                    out.push_str(&a);
                }
                out.push(')');
                return Ok(out);
            }
        }
        let name = crate::materialize::static_class_name(&self.st, ty)?;
        let mut out = String::from("(ty ");
        quote_into(&mut out, &name);
        out.push(')');
        Ok(out)
    }

    /// A top-level binary object's singleton is resolved as a module, never
    /// widened to its module class. Instance and source paths need identities
    /// that a runtime mirror's staticModule lookup cannot supply.
    pub(crate) fn binary_module_type_wire(&mut self, ty: &Type) -> Option<String> {
        let id = match ty {
            Type::ModuleRef(id) => *id,
            Type::Class { sym, args } if args.is_empty() => *sym,
            Type::SingleType { sym, .. } => *sym,
            _ => return None,
        };
        let id = self.st.module_class_of(id);
        if self.is_current_run_class(id) {
            return None;
        }
        let s = self.st.get(id);
        if s.kind != SymKind::ModuleClass || self.st.get(s.owner).kind != SymKind::Package {
            return None;
        }
        let name = s.jvm_name.strip_suffix('$')?;
        if name.is_empty() || name.contains('$') {
            return None;
        }
        Some(format!("(mod {})", quoted(&name.replace('/', "."))))
    }

    /// Class-owned binary parameters retain their identity in the runtime mirror.
    pub(crate) fn binary_type_param_wire(&mut self, ty: &Type) -> Result<Option<String>, String> {
        if let Type::TypeParam(id) = ty {
            let owner = self.st.get(*id).owner;
            if !owner.is_none() && self.st.get(owner).is_class_like() && !self.is_current_run_class(owner) {
                if let Some(index) = self.st.get(owner).tparams.iter().position(|p| p == id) {
                    let name = crate::materialize::static_class_of_sym(&self.st, owner)?;
                    return Ok(Some(format!("(param {} {index})", quoted(&name))));
                }
            }
        }
        Ok(None)
    }

    /// Preserve source type identities and arguments in both directions.
    /// Type parameters are weak types with their declared bounds, not their
    /// erasures or a runtime ClassTag approximation.
    pub(crate) fn source_type_wire(&mut self, ty: &Type) -> Result<Option<String>, String> {
        if let Type::Applied { ctor, args } = ty {
            if let Some(mut wire) = self.source_type_wire(ctor)? {
                wire.pop();
                for arg in args { wire.push(' '); wire.push_str(&self.type_to_wire(arg)?); }
                wire.push(')');
                return Ok(Some(wire));
            }
        }
        let (id, args) = match ty {
            Type::Class { sym, args }
                if self.st.get(*sym).kind == SymKind::Class && self.is_current_run_class(*sym) =>
            {
                (*sym, args.as_slice())
            }
            Type::TypeParam(id) => {
                let owner = self.st.get(*id).owner;
                let mut enclosing = owner;
                while !enclosing.is_none() && !self.st.get(enclosing).is_class_like() {
                    enclosing = self.st.get(enclosing).owner;
                }
                let source_class_parameter = !enclosing.is_none()
                    && self.st.get(owner).tparams.contains(id)
                    && self.is_current_run_class(enclosing);
                if !self.tparam_in_scope(*id) && !source_class_parameter {
                    return Ok(None);
                }
                (*id, &[][..])
            }
            _ => return Ok(None),
        };
        let mut wire = format!("(src {}", id.0);
        for arg in args {
            wire.push(' ');
            wire.push_str(&self.type_to_wire(arg)?);
        }
        wire.push(')');
        Ok(Some(wire))
    }

    /// Whether `sym` is a class this compilation run is itself defining --
    /// that is, one with no class file for the engine's mirror to find. The
    /// same test [`Typer::tag_wire`] makes, and for the same reason.
    pub(crate) fn is_current_run_class(&mut self, sym: SymbolId) -> bool {
        let jvm = self.st.jvm_internal(sym);
        !jvm.is_empty() && !matches!(self.binary.find_class(&jvm), Ok(Some(_)))
    }
}

/// Write a tree the typer has produced in the shape the engine rebuilds.
///
/// This is the answer half of the bridge, and it is deliberately narrower
/// than what the typer can build: a node with no faithful reflect spelling is
/// refused by name rather than approximated, exactly the way
/// `Typer::tree_from_reply` refuses in the other direction. An approximation
/// here would be a tree the implementation then *splices into its expansion*.
fn answer_tree_to_wire(cx: &WireCx, t: &Tree, out: &mut String) -> Result<(), String> {
    let start = out.len();
    answer_tree_to_wire_body(cx, t, out)?;
    super::expand::mirror_tree_identity(cx.st, t, start, out);
    Ok(())
}

fn answer_tree_to_wire_body(cx: &WireCx, t: &Tree, out: &mut String) -> Result<(), String> {
    match &t.kind {
        TreeKind::Literal { lit } => {
            out.push_str("(t \"Literal\" (s0) ");
            lit_to_wire(lit, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Ident { name } => {
            // A name that resolved to a member of an enclosing class means
            // `C.this.name` in a typed tree, the same way `c.prefix` carries
            // one; a name that resolved to nothing keeps its own spelling.
            match owner_qualifier(cx.st, t.sym) {
                Some(owner) => {
                    out.push_str("(t \"Select\" (s0) (t \"This\" (s0) (n type ");
                    quote_into(out, &owner);
                    out.push_str(")) (n term ");
                    quote_into(out, &encode_method_name(name));
                    out.push_str("))");
                }
                None => {
                    out.push_str("(t \"Ident\" (s0) (n term ");
                    quote_into(out, &encode_method_name(name));
                    out.push_str("))");
                }
            }
            Ok(())
        }
        TreeKind::This { qual } => {
            out.push_str("(t \"This\" (s0) (n type ");
            quote_into(out, qual.as_deref().unwrap_or(""));
            out.push_str("))");
            Ok(())
        }
        TreeKind::Select { qual, name } => {
            out.push_str("(t \"Select\" (s0) ");
            answer_tree_to_wire(cx, qual, out)?;
            out.push_str(" (n term ");
            quote_into(out, &encode_method_name(name));
            out.push_str("))");
            Ok(())
        }
        TreeKind::Apply { fun, args } => {
            out.push_str("(t \"Apply\" (s0) ");
            if matches!(fun.kind, TreeKind::New { .. }) {
                super::expand::application_fun_to_wire(cx, fun, out)?;
            } else {
                answer_tree_to_wire(cx, fun, out)?;
            }
            out.push_str(" (l");
            for a in args {
                out.push(' ');
                answer_tree_to_wire(cx, a, out)?;
            }
            out.push_str("))");
            Ok(())
        }
        TreeKind::Block { stats, expr } => {
            out.push_str("(t \"Block\" (s0) (l");
            for s in stats {
                out.push(' ');
                answer_tree_to_wire(cx, s, out)?;
            }
            out.push_str(") ");
            answer_tree_to_wire(cx, expr, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::If { cond, thenp, elsep } => {
            out.push_str("(t \"If\" (s0) ");
            answer_tree_to_wire(cx, cond, out)?;
            out.push(' ');
            answer_tree_to_wire(cx, thenp, out)?;
            out.push(' ');
            answer_tree_to_wire(cx, elsep, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Empty => {
            out.push_str("(t \"EmptyTree\" (s0))");
            Ok(())
        }
        _ => super::expand::tree_to_wire(cx, t, out).map_err(|why| format!("a tree: {why}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::TypecheckOptions;

    fn atom(value: &str) -> Sexp {
        Sexp::Atom(value.to_string())
    }

    fn string(value: &str) -> Sexp {
        Sexp::Str(value.to_string())
    }

    fn infer_question(wanted: Sexp, silent: &str, no_macros: &str) -> Vec<Sexp> {
        vec![
            atom("q"),
            atom("inferImplicitValue"),
            wanted,
            atom(silent),
            atom(no_macros),
        ]
    }

    #[test]
    fn infer_query_rejects_malformed_flags_and_length() {
        let mut typer = Typer::new(0, &TypecheckOptions::default());
        let wanted = Sexp::List(vec![atom("ty"), string("scala.Int")]);
        let malformed_flag = typer.answer_query(&infer_question(wanted, "true", "0"));
        assert!(malformed_flag.contains("malformed") && malformed_flag.contains("silent"));

        let short = vec![atom("q"), atom("inferImplicitValue")];
        assert!(typer.answer_query(&short).contains("malformed"));
    }

    #[test]
    fn infer_query_rolls_back_failed_type_resolution_and_honours_depth_limit() {
        let mut typer = Typer::new(0, &TypecheckOptions::default());
        let before = typer.diags.len();
        let missing = Sexp::List(vec![atom("ty"), string("no.such.Missing")]);
        let answer = typer.answer_query(&infer_question(missing, "1", "0"));
        assert!(answer.starts_with("(no "), "{answer}");
        assert_eq!(typer.diags.len(), before);

        typer.macro_query_depth = MAX_QUERY_DEPTH;
        let wanted = Sexp::List(vec![atom("ty"), string("scala.Int")]);
        let answer = typer.answer_query(&infer_question(wanted, "1", "0"));
        assert!(
            answer.contains("more than") && answer.contains("deep"),
            "{answer}"
        );
        assert_eq!(typer.macro_query_depth, MAX_QUERY_DEPTH);
    }

    #[test]
    fn typecheck_query_rolls_back_failed_tree_reconstruction() {
        let mut typer = Typer::new(0, &TypecheckOptions::default());
        let before = typer.diags.len();
        let returned_tree = Sexp::List(vec![
            atom("t"),
            string("TypeTree"),
            Sexp::List(vec![
                atom("tm"),
                string("no.such.MissingOwner"),
                string("Type"),
            ]),
            Sexp::List(vec![atom("ty"), string("scala.Int")]),
        ]);
        let question = vec![
            atom("q"),
            atom("typecheck"),
            returned_tree,
            string("TYPE"),
            atom("1"),
        ];
        let answer = typer.answer_query(&question);
        assert!(answer.starts_with("(no "), "{answer}");
        assert_eq!(typer.diags.len(), before);

        let mut malformed = question;
        malformed[4] = atom("true");
        assert!(typer.answer_query(&malformed).contains("malformed"));
    }
}

/// The `C` of the `C.this` a typed `Ident` naming a member of `C` stands for.
///
/// Only a *class* owner: a name owned by a method is a local, and a local has
/// no path at all -- writing `C.this.x` for one would be a different tree.
fn owner_qualifier(st: &SymbolTable, sym: SymbolId) -> Option<String> {
    crate::expand::this_qualifier_of(st, sym)
}

/// The flags of a class this run is compiling, by name.
///
/// Names rather than a number: nsc's bit layout is an internal detail and
/// several bits carry two names -- exactly what the engine's `Modifiers`
/// serialiser already argues in the other direction. Only the flags that
/// change what a *question about the class* answers travel: `isCaseClass`,
/// `isTrait`, `isAbstract`, `isFinal`, `isSealed`.
fn class_flags_wire(flags: Flags) -> String {
    let mut out = String::from(" (f");
    for (bit, name) in [
        (Flags::CASE, "CASE"),
        (Flags::TRAIT, "TRAIT"),
        (Flags::ABSTRACT, "ABSTRACT"),
        (Flags::FINAL, "FINAL"),
        (Flags::SEALED, "SEALED"),
        (Flags::INTERFACE, "INTERFACE"),
        (Flags::SYNTHETIC, "SYNTHETIC"),
        (Flags::PRIVATE, "PRIVATE"),
        (Flags::PROTECTED, "PROTECTED"),
        (Flags::LOCAL, "LOCAL"),
    ] {
        if flags.contains(bit) {
            out.push(' ');
            quote_into(&mut out, name);
        }
    }
    out.push(')');
    out
}

/// The flags of one declared member.
///
/// `DEFERRED` is the one that must never be wrong: a member reported as
/// concrete when it is abstract changes what an implementation may build with
/// it. `Flags::ABSTRACT` on a member is scala-rs's spelling for a declaration
/// with no body, which is nsc's `DEFERRED`.
fn member_flags_wire(flags: Flags, kind: SymKind, ctor: bool) -> String {
    let mut out = String::from(" (f");
    if flags.contains(Flags::ABSTRACT) {
        out.push_str(" \"DEFERRED\"");
    }
    if matches!(kind, SymKind::Method) {
        out.push_str(" \"METHOD\"");
    }
    if ctor {
        out.push_str(" \"CONSTRUCTOR\"");
    }
    for (bit, name) in [
        (Flags::PARAM, "PARAM"),
        (Flags::COVARIANT, "COVARIANT"),
        (Flags::CONTRAVARIANT, "CONTRAVARIANT"),
        (Flags::DEFAULTPARAM, "DEFAULTPARAM"),
        (Flags::LOCAL, "LOCAL"),
        (Flags::PRIVATE, "PRIVATE"),
        (Flags::PROTECTED, "PROTECTED"),
        (Flags::FINAL, "FINAL"),
        (Flags::IMPLICIT, "IMPLICIT"),
        (Flags::LAZY, "LAZY"),
        (Flags::MUTABLE, "MUTABLE"),
        (Flags::OVERRIDE, "OVERRIDE"),
        (Flags::SYNTHETIC, "SYNTHETIC"),
    ] {
        if flags.contains(bit) {
            out.push(' ');
            quote_into(&mut out, name);
        }
    }
    out.push(')');
    out
}
