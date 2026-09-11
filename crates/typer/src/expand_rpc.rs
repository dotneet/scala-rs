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

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_pickle::names::encode_method_name;
use scala_rs_span::Level;

use crate::check::Typer;
use crate::expand::{at, lit_to_wire, quote_into, scala_full_name, Sexp, WireCx};
use crate::symbol::{SymKind, SymbolTable};

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
                None => refusal("the function has not been typed at this call site"),
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
        if s.is_class_like() {
            return self.mirror_class_info(sym);
        }
        if !s.tparams.is_empty() {
            return Err(format!(
                "mirror info for polymorphic symbol {} is not implemented",
                s.name
            ));
        }
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
                    args: Vec::new(),
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
                return Ok(format!("(nullary {info})"));
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
            return Ok(info);
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
        let (Ok(tree_sexp), Ok(mode)) = (at(items, 2), at(items, 3).map(|s| s.text())) else {
            // Unreachable through the engine, which always writes all three;
            // said rather than unwrapped, because a malformed line must not
            // leave the engine blocked on a read.
            return refusal("the macro engine asked a malformed `c.typecheck`");
        };
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
        let mut tree = match self.tree_from_reply(tree_sexp, span) {
            Ok(t) => t,
            Err(why) => return refusal(&why),
        };
        // A macro application inside the tree is refused (`macro_engine_busy`),
        // and the refusal is recorded against the span every node of a rebuilt
        // tree carries -- which is the *outer* call site's. Left there it would
        // be a reason attached to a call that succeeded, so it is put back the
        // way it was, exactly like the diagnostics.
        let key = self.macro_failure_key(span);
        let outer_failure = self.macro_failures.get(&key).cloned();
        self.macro_query_depth += 1;
        let answer = match mode.as_str() {
            "TERM" => self.typecheck_term(&mut tree),
            "TYPE" => self.typecheck_type(&tree),
            other => refusal(&format!(
                "`c.typecheck` was asked for {other}mode, which scala-rs does \
                 not implement (only TERMmode and TYPEmode)"
            )),
        };
        self.macro_query_depth -= 1;
        match outer_failure {
            Some(why) => {
                self.macro_failures.insert(key, why);
            }
            None => {
                self.macro_failures.remove(&key);
            }
        }
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
        let types = crate::expand::WireTypes::default();
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
    /// `(ty "a.b.C" <arg>…)` is a class its mirror finds on the macro
    /// classpath, applied to its type arguments written the same way;
    /// `(cst …)` is the constant type nsc gives a literal, which
    /// `c.typecheck(Literal(Constant(1))).tpe` has to be if it is to be the
    /// type nsc reports; and `(syn "a.b.C")` is **a class this run is
    /// compiling**.
    ///
    /// That last one is the mirror. A class the current run defines has no
    /// class file, so `mirror.staticClass` can never find it and the engine
    /// cannot be told about it by name resolution at all -- the same wall
    /// `docs/macros.md` §5.1 hits for a type *argument*, reached here from
    /// the other side: it is the answer to a question the implementation
    /// asked. The type is remembered under that name so a later mention of it
    /// in the expansion is read back as the `Type` the typer already has
    /// rather than resolved again (`Typer::macro_local_tags`).
    ///
    /// Everything else is refused by name. A singleton type, a refinement or
    /// an abstract type has no faithful spelling on the other side, and an
    /// approximate one would have the implementation reasoning about a type
    /// that is not the one it asked about.
    pub(crate) fn type_to_wire(&mut self, ty: &Type) -> Result<String, String> {
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
            if matches!(self.st.get(sym).kind, SymKind::Class) && self.is_current_run_class(sym) {
                let full = scala_full_name(&self.st, sym);
                if !args.is_empty() {
                    return Err(format!(
                        "`{full}`, a class this run is compiling applied to type \
                         arguments; the engine is given such a class as its \
                         identity, and its type parameters would have nothing to bind"
                    ));
                }
                self.macro_local_tags.insert(full.clone(), ty.clone());
                return Ok(format!("(src {})", sym.0));
            }
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

    /// Whether `sym` is a class this compilation run is itself defining --
    /// that is, one with no class file for the engine's mirror to find. The
    /// same test [`Typer::tag_descriptor`] makes, and for the same reason.
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
    super::expand::mirror_tree_identity(t, start, out);
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
