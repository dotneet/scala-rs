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
        "parse" => "parse",
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
            let externally_loadable = {
                let s = self.st.get(sym);
                (s.is_class_like() || s.kind == SymKind::Module) && !self.is_current_run_class(sym)
            };
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
                "(a symbol {} {} {} {} {}{} {} (shape {}{}) (erased{}))",
                id,
                quoted(&format!("{:?}", s.kind)),
                quoted(&s.name),
                quoted(&full),
                owner.0,
                if s.is_class_like() {
                    class_flags_wire(class_flags)
                } else {
                    member_flags_wire(s.flags, s.kind, s.name == "<init>")
                },
                quoted(if externally_loadable { &s.jvm_name } else { "" }),
                s.tparams.len(),
                s.paramss
                    .iter()
                    .map(|ps| format!(" {}", ps.len()))
                    .collect::<String>(),
                crate::pickle_supply::flat_erased_params(&self.st, &s.ty)
                    .iter()
                    .map(|desc| format!(" {}", quoted(desc.as_deref().unwrap_or(""))))
                    .collect::<String>()
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
        // Reflection forces a method's pending inferred signature just as a
        // source selection does. Only an inference already in progress is a
        // recursive definition; an unvisited method is still completable.
        if matches!(&self.st.get(sym).ty, Type::Method { ret, .. } if ret.is_no_type())
            && !self.lazy_completing.contains(&sym)
        {
            self.complete_lazy_sig(sym, self.macro_rpc_span);
        }
        let s = self.st.get(sym).clone();
        if s.kind == SymKind::TypeParam {
            let lo = self.type_to_wire(s.bound_lo.as_ref().unwrap_or(&Type::Nothing))?;
            let hi = self.type_to_wire(s.bound_hi.as_ref().unwrap_or(&Type::Any))?;
            let bounds = format!("(bounds {lo} {hi})");
            return Ok(if s.tparams.is_empty() {
                bounds
            } else {
                format!(
                    "(poly (params {}) {bounds})",
                    s.tparams
                        .iter()
                        .map(|p| p.0.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            });
        }
        if s.is_class_like() {
            return self.mirror_class_info(sym);
        }
        if s.kind == SymKind::TypeMember {
            let info = if s.is_type_alias {
                self.type_to_wire(&s.ty)?
            } else {
                format!(
                    "(bounds {} {})",
                    self.type_to_wire(s.bound_lo.as_ref().unwrap_or(&Type::Nothing))?,
                    self.type_to_wire(s.bound_hi.as_ref().unwrap_or(&Type::Any))?
                )
            };
            return Ok(if s.tparams.is_empty() {
                info
            } else {
                format!(
                    "(poly (params {}) {info})",
                    s.tparams
                        .iter()
                        .map(|p| p.0.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            });
        }
        let polymorphic = |info: String| {
            if s.tparams.is_empty() {
                info
            } else {
                format!(
                    "(poly (params {}) {info})",
                    s.tparams
                        .iter()
                        .map(|p| p.0.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
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
                    let wired = self.type_to_wire(&ty).map_err(|e| {
                        format!("{e} in {} parameter {}", s.name, self.st.get(param).name)
                    })?;
                    if self.st.get(param).owner != sym {
                        // A parameter whose symbol belongs to something else:
                        // a case class's constructor shares its fields'. It
                        // travels by name and flags, as a parameter of its
                        // own on the far side, rather than as the field.
                        let mut named = String::from("(argn ");
                        quote_into(&mut named, &self.st.get(param).name);
                        named.push_str(" (f \"PARAM\"");
                        for (flag, name) in [
                            (Flags::DEFAULTPARAM, "DEFAULTPARAM"),
                            (Flags::IMPLICIT, "IMPLICIT"),
                        ] {
                            if self.st.get(param).flags.contains(flag) {
                                named.push(' ');
                                quote_into(&mut named, name);
                            }
                        }
                        named.push_str(") ");
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
            "parse" => self.answer_parse(items),
            "inferImplicitValue" => self.answer_infer_implicit_value(items),
            "enclosingOwner" => format!("(a ref {})", self.macro_current_owner().0),
            "openImplicits" => self.answer_open_implicits(),
            "isAccessible" | "isAccessibleMember" => self.answer_accessible(items),
            "resetImplicits" => {
                self.invalidate_implicit_caches();
                String::from("(a reset)")
            }
            "functionSymbol" | "symbol" | "symbolInfo" | "companion" | "modulePair" => {
                self.answer_mirror_symbol(items)
            }
            "viewInfo" => self.answer_view_info(items),
            other => refusal(&format!(
                "the macro engine asked scala-rs `{other}`, which it does not answer"
            )),
        }
    }

    fn answer_accessible(&mut self, items: &[Sexp]) -> String {
        let result = (|| -> Result<bool, String> {
            let (symbols, prefix_wire) = if at(items, 1)?.text() == "isAccessible" {
                let id = SymbolId(
                    at(items, 2)?
                        .text()
                        .parse::<u32>()
                        .map_err(|e| e.to_string())?,
                );
                if id.is_none() || id.0 as usize >= self.st.symbols.len() {
                    return Err("unknown accessibility symbol identity".into());
                }
                (vec![id], at(items, 3)?)
            } else {
                let owner = self.query_type_from_wire(at(items, 2)?, self.macro_rpc_span)?;
                let owner = self
                    .st
                    .class_sym_of(&owner)
                    .ok_or("accessibility owner is not a class")?;
                let name = scala_rs_pickle::names::decode_method_name(&at(items, 3)?.text());
                self.complete_binary_member(owner, &name, self.macro_rpc_span);
                (self.st.lookup_member(owner, &name), at(items, 4)?)
            };
            let ty = self.query_type_from_wire(prefix_wire, self.macro_rpc_span)?;
            let mut prefix = Tree::new(NodeId(0), self.macro_rpc_span, TreeKind::Empty);
            prefix.ty = ty;
            let first = *symbols
                .first()
                .ok_or("accessibility member was not found")?;
            let accessible = self.accessible(first, Some(&prefix));
            if symbols
                .iter()
                .any(|&id| self.accessible(id, Some(&prefix)) != accessible)
            {
                return Err("overloaded members have different accessibility; exact symbol identity required".into());
            }
            Ok(accessible)
        })();
        match result {
            Ok(accessible) => format!("(a accessible {accessible})"),
            Err(why) => refusal(&why),
        }
    }

    fn answer_open_implicits(&mut self) -> String {
        let candidates = self.building_implicits.clone();
        let mut out = String::from("(a implicits");
        for (id, wanted) in candidates.into_iter().rev() {
            let origin = self
                .implicit_instance_origins
                .get(&id)
                .copied()
                .unwrap_or(id);
            let reference = self.ref_implicit_with_receiver(origin, self.macro_rpc_span);
            let prefix = match &reference.kind {
                TreeKind::Select { qual, .. } => match self.type_to_wire(&qual.ty) {
                    Ok(t) => t,
                    Err(why) => return refusal(&format!("open implicit prefix: {why}")),
                },
                _ => String::from("(noprefix)"),
            };
            let wanted = match self.type_to_wire(&wanted) {
                Ok(t) => t,
                Err(why) => return refusal(&format!("open implicit expected type: {why}")),
            };
            let mut tree = String::new();
            let types = crate::expand::WireTypes::default();
            let cx = WireCx {
                st: &self.st,
                types: &types,
            };
            if let Err(why) = answer_tree_to_wire(&cx, &reference, &mut tree) {
                return refusal(&format!("open implicit reference: {why}"));
            }
            out.push_str(&format!(" ({prefix} {} {wanted} {tree})", origin.0));
        }
        out.push(')');
        out
    }

    fn answer_parse(&self, items: &[Sexp]) -> String {
        let Ok(source) = at(items, 2) else {
            return refusal("malformed parse request");
        };
        let file = scala_rs_span::SourceFile::new("<macro>", source.text());
        let parsed = scala_rs_parser::parse::parse_snippet(&file, self.file_index);
        if let Some(error) = parsed.diags.iter().find(|d| d.level == Level::Error) {
            return format!("(a fail {})", quoted(&error.message));
        }
        let types = crate::expand::WireTypes::default();
        let cx = WireCx {
            st: &self.st,
            types: &types,
        };
        let mut tree = String::new();
        match answer_tree_to_wire(&cx, &parsed.tree, &mut tree) {
            Ok(()) => format!("(a parsed {tree})"),
            Err(why) => refusal(&format!("c.parse produced {why}")),
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
            this.with_whitebox_fits(span, |this| {
                this.answer_infer_implicit_search(&pt, silent, mark)
            })
        })
    }

    fn answer_infer_implicit_search(&mut self, pt: &Type, silent: bool, mark: usize) -> String {
        let span = self.macro_rpc_span;
        // A macro query starts a fresh implicit search, but its selected tree
        // is materialized while the enclosing implicit macro is still being
        // built. Use a detached instance for each nested query level so a
        // derivation rule used by both the enclosing and requested evidence
        // is not mistaken for the very same in-progress application. The
        // ordinary open-implicit stack still rejects genuinely divergent
        // searches by declaration origin and target type.
        let depth = self.macro_query_depth;
        let mut rejected = Vec::new();
        loop {
            let search = self.search_macro_implicit(pt, depth, &rejected);
            match search {
                crate::implicits::ImplicitSearch::Found(id) => {
                    // `implicit_tree` expands a selected implicit macro through a
                    // nested conversation. Never serialize an unexpanded reference
                    // as successful evidence when that nested expansion failed.
                    let attempt_mark = self.diags.len();
                    let key = self.macro_failure_key(span);
                    let (mut tree, _nested_failure) = self
                        .with_isolated_macro_failure(key, |this| {
                            this.implicit_tree(id, pt, span, depth)
                        });
                    self.adapt(&mut tree, pt);
                    // Ordinary implicit methods can contain failed macro
                    // evidence too. Returning those calls as a successful
                    // witness lets an enclosing macro re-expand them under
                    // a different open-implicit stack.
                    self.report_macro_calls(&tree);
                    // A nested Lazy-style derivation can deliberately return a
                    // reference to a val that the enclosing macro will place in
                    // its final block. nsc keeps that unbound intermediate tree
                    // typed; the reference only becomes lexically visible after
                    // the outer expansion resumes. Preserve the already fitted
                    // result for that exact generated-name case, while retaining
                    // every other diagnostic from the nested macro.
                    let deferred_outer_local = self.diags[attempt_mark..]
                        .iter()
                        .filter(|d| d.level == Level::Error)
                        .next()
                        .is_some()
                        && self.diags[attempt_mark..]
                            .iter()
                            .filter(|d| d.level == Level::Error)
                            .all(|d| d.message.starts_with("not found: value inst$macro$"));
                    let failures = if deferred_outer_local {
                        self.diags.truncate(attempt_mark);
                        None
                    } else {
                        self.take_probe_errors(attempt_mark)
                    };
                    if tree.ty.is_error() || failures.is_some() {
                        // Implicit search is transactional through materialization:
                        // a candidate whose macro (or one of its evidence macros)
                        // aborts is discarded and the next applicable candidate is
                        // tried. This is how a specialized derivation can decline a
                        // type and let a lower-priority generic derivation handle it.
                        let origin = self
                            .implicit_instance_origins
                            .get(&id)
                            .copied()
                            .unwrap_or(id);
                        if rejected
                            .iter()
                            .any(|(prior, wanted)| *prior == origin && wanted == pt)
                        {
                            self.diags.truncate(mark);
                            if !silent {
                                self.error(
                                    span,
                                    failures
                                        .unwrap_or_else(|| self.missing_implicit_message(pt, None)),
                                );
                            }
                            return "(a none)".to_string();
                        }
                        rejected.push((origin, pt.clone()));
                        continue;
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
                    // A nested implicit macro can return generated TypeTrees that
                    // have no source spelling. Serialize them with the same
                    // resolved-type side table as a top-level expansion; an empty
                    // table rejected the otherwise valid witness while handing it
                    // back to the outer macro.
                    let mut types = crate::expand::WireTypes::default();
                    self.collect_wire_types(&tree, &mut types);
                    let cx = crate::expand::WireCx {
                        st: &self.st,
                        types: &types,
                    };
                    return match answer_tree_to_wire(&cx, &tree, &mut built) {
                        Ok(()) => format!("(a ok {ty} {built})"),
                        Err(why) => refusal(&format!("`c.inferImplicitValue` produced {why}")),
                    };
                }
                crate::implicits::ImplicitSearch::None => {
                    self.diags.truncate(mark);
                    if !silent {
                        self.error(span, self.missing_implicit_message(pt, None));
                    }
                    return "(a none)".to_string();
                }
                crate::implicits::ImplicitSearch::Ambiguous(ids) => {
                    self.diags.truncate(mark);
                    if !silent {
                        self.error(
                            span,
                            format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                        );
                    }
                    return "(a none)".to_string();
                }
            }
        }
    }

    fn search_macro_implicit(
        &mut self,
        pt: &Type,
        depth: usize,
        rejected: &[(SymbolId, Type)],
    ) -> crate::implicits::ImplicitSearch {
        let saved_open = self.open_implicits.borrow().clone();
        self.open_implicits.borrow_mut().extend_from_slice(rejected);
        self.invalidate_implicit_caches();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            *self.diverged_implicit.borrow_mut() = None;
            let mut search = self.retry_whitebox_fits(|this| this.search_implicit_at(pt, depth));
            if matches!(search, crate::implicits::ImplicitSearch::None)
                && self.warm_implicit_candidates(std::slice::from_ref(pt))
            {
                search = self.retry_whitebox_fits(|this| this.search_implicit_at(pt, depth));
            }
            search
        }));
        *self.open_implicits.borrow_mut() = saved_open;
        self.invalidate_implicit_caches();
        match result {
            Ok(search) => search,
            Err(payload) => std::panic::resume_unwind(payload),
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
        let mut base = if self.st.get(member).kind == SymKind::TypeParam {
            Type::TypeParam(member)
        } else {
            Type::TypeMember(member)
        };
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
        let owner = self.as_type_owner(owner);
        if let Some(parameter) = self
            .st
            .get(owner)
            .tparams
            .iter()
            .copied()
            .find(|p| self.st.get(*p).name == member_name)
        {
            return Ok(parameter);
        }
        self.complete_binary_member(owner, member_name, span);
        self.pickle
            .complete_type_member(&mut self.st, &mut self.binary, owner, member_name);
        self.pickle
            .completed_type_member_decl(owner, member_name)
            .or_else(|| {
                self.st
                    .lookup_member(owner, member_name)
                    .into_iter()
                    .find(|id| self.st.get(*id).kind == SymKind::TypeMember)
            })
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
        // Nested implementations share the conversation stack. Bound the
        // recursive query chain even when each individual request terminates.
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
        // A macro application inside the tree may fail during its nested conversation,
        // and the refusal is recorded against the span every node of a rebuilt
        // tree carries -- which is the *outer* call site's. Left there it would
        // be a reason attached to a call that succeeded, so it is put back the
        // way it was, exactly like the diagnostics.
        let key = self.macro_failure_key(span);
        let (answer, _nested_failure) = self.with_isolated_macro_failure(key, |this| {
            this.with_macro_query_depth(|this| {
                let depth = this.macro_query_depth;
                this.with_implicit_search_depth(depth, |this| match mode.as_str() {
                    "TERM" => this.typecheck_term(&mut tree),
                    "TYPE" => this.typecheck_type(&tree),
                    other => refusal(&format!(
                        "`c.typecheck` was asked for {other}mode, which scala-rs does \
                         not implement (only TERMmode and TYPEmode)"
                    )),
                })
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
    pub(crate) fn take_probe_errors(&mut self, mark: usize) -> Option<String> {
        let first = self.diags[mark..]
            .iter()
            .find(|d| d.level == Level::Error)
            .map(|d| d.message.clone());
        self.diags.truncate(mark);
        first
    }
}

/// Bound re-entrant `c.typecheck` and `c.inferImplicitValue` queries while
/// allowing nested product derivations, which ask again for each field.
const MAX_QUERY_DEPTH: usize = 64;

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
        if let Type::Repeated(element) = ty {
            return Ok(format!("(repeated {})", self.type_to_wire(element)?));
        }
        if matches!(ty, Type::AnyRef) {
            return Ok("(ty \"java.lang.Object\")".into());
        }
        if let Type::TypeParam(id) = ty {
            if self.st.get(self.st.get(*id).owner).kind == SymKind::TypeMember {
                return Ok(format!("(param {})", id.0));
            }
        }
        if let Type::Applied { .. } = ty {
            let expanded = self.st.expand_applied_hk_alias(ty.clone());
            if expanded != *ty {
                return self.type_to_wire(&expanded);
            }
        }
        if let Type::TypeMember(id) = ty {
            let s = self.st.get(*id);
            if s.is_type_alias && s.tparams.is_empty() && s.ty != *ty {
                return self.type_to_wire(&s.ty.clone());
            }
        }
        if let Type::Refined { parents, decls } = ty {
            let key = format!("<macro-type-{}>", self.macro_local_tags.len());
            self.macro_local_tags.insert(key.clone(), ty.clone());
            let mut out = format!("(refined {} (parents", quoted(&key));
            for p in parents {
                out.push(' ');
                out.push_str(&self.type_to_wire(p)?);
            }
            out.push_str(") (members");
            for decl in decls {
                let scala_rs_parser::RefineDecl::Type {
                    name,
                    rhs,
                    tparams,
                    lo,
                    hi,
                } = decl
                else {
                    return Err("a refinement with term members".into());
                };
                out.push_str(&format!(" (member {} ", quoted(name)));
                if let Some(rhs) = rhs {
                    if *tparams > 0 {
                        let Type::TypeMember(id) = rhs else {
                            return Err("a captured refinement type lambda".into());
                        };
                        let member = self.st.get(*id).clone();
                        out.push_str("(poly (params");
                        for tp in member.tparams {
                            let s = self.st.get(tp).clone();
                            out.push_str(&format!(
                                " ({} {} {} {})",
                                tp.0,
                                quoted(&s.name),
                                self.type_to_wire(s.bound_lo.as_ref().unwrap_or(&Type::Nothing))?,
                                self.type_to_wire(s.bound_hi.as_ref().unwrap_or(&Type::Any))?
                            ));
                        }
                        out.push_str(") ");
                        out.push_str(&self.type_to_wire(&member.ty)?);
                        out.push(')');
                    } else {
                        out.push_str(&self.type_to_wire(rhs)?);
                    }
                } else {
                    out.push_str(&format!(
                        "(bounds {} {})",
                        self.type_to_wire(lo.as_ref().unwrap_or(&Type::Nothing))?,
                        self.type_to_wire(hi.as_ref().unwrap_or(&Type::Any))?
                    ));
                }
                out.push(')');
            }
            out.push_str("))");
            return Ok(out);
        }
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
                && jvm
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.contains('$'))
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
            let name = crate::materialize::static_class_of_sym(&self.st, sym)
                .or_else(|why| {
                    let mut owner = self.st.get(sym).owner;
                    while !owner.is_none() {
                        let s = self.st.get(owner);
                        if !matches!(
                            s.kind,
                            SymKind::Package | SymKind::Module | SymKind::ModuleClass
                        ) {
                            return Err(why);
                        }
                        owner = s.owner;
                    }
                    Ok(scala_rs_pickle::names::nested_to_dotted(
                        &self.st.jvm_internal(sym).replace('/', "."),
                    ))
                })
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
            if !owner.is_none()
                && self.st.get(owner).is_class_like()
                && !self.is_current_run_class(owner)
            {
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
                for arg in args {
                    wire.push(' ');
                    wire.push_str(&self.type_to_wire(arg)?);
                }
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
            // An unapplied alias is a type constructor, not an abstract
            // parameter requiring a runtime tag. Keep its declaration identity
            // so the mirror sees the alias's binders and right-hand side.
            Type::TypeMember(id) if self.st.get(*id).is_type_alias => (*id, &[][..]),
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
            // An implicit synthesized for another macro is resolved in the
            // implicit's scope, then spliced at the outer macro call site.
            // A companion member such as `optionEvidence(baseEvidence)` is
            // not necessarily imported there, so retain the symbol's stable
            // path just as typed `c.prefix` transport does.
            // Binary members also carry the same-run symbol identity. The JVM
            // macro can duplicate or untypecheck the tree and still return the
            // member identity, instead of reducing a qualified helper call to
            // an unresolvable bare identifier.
            if let Some(path) = super::expand::static_member_path(cx.st, t.sym) {
                root_path_with_source_sym_to_wire(&path, t.sym, out);
                return Ok(());
            }
            if let Some(path) = super::expand::static_module_path(cx.st, t.sym) {
                super::expand::root_path_to_wire(&path, out);
                return Ok(());
            }
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

fn root_path_with_source_sym_to_wire(path: &str, sym: SymbolId, out: &mut String) {
    let segments = path.split('.').collect::<Vec<_>>();
    let mut built = String::from("(t \"Ident\" (s0) (n term \"_root_\"))");
    for (index, segment) in segments.iter().enumerate() {
        let mut next = if index + 1 == segments.len() {
            format!("(t \"Select\" (srm {}) ", sym.0)
        } else {
            String::from("(t \"Select\" (s0) ")
        };
        next.push_str(&built);
        next.push_str(" (n term ");
        quote_into(&mut next, &encode_method_name(segment));
        next.push_str("))");
        built = next;
    }
    out.push_str(&built);
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
