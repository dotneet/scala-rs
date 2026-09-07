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
use crate::expand::{at, lit_to_wire, quote_into, scala_full_name, Sexp};
use crate::symbol::{SymKind, SymbolTable};

impl Typer {
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
        match answer_tree_to_wire(&self.st, tree, &mut built) {
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
                         arguments; the placeholder the engine is given carries a \
                         name and nothing else"
                    ));
                }
                self.macro_local_tags.insert(full.clone(), ty.clone());
                return Ok(self.run_class_wire(sym, &full));
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

    /// A class this run is compiling, described for the engine's mirror.
    ///
    /// This is `docs/macros.md` §7.18's step 1: the engine's symbol for a
    /// current-run class is filled in *by asking scala-rs*, at the moment the
    /// class is first named to it. Before this, such a class travelled as a
    /// name and nothing else (`(syn …)`, §5.1), and an implementation that
    /// asked it any question at all -- even `tpe.toString`, which needs the
    /// symbol's info -- got an `AssertionError` out of the reflect internals.
    ///
    /// **It is all or nothing.** Either scala-rs can describe the class
    /// completely and truthfully right now -- every parent, every declared
    /// member, every one of their types -- or the class travels as the empty
    /// placeholder it always did. A partial description is the one thing that
    /// must not happen: a `decls` missing a member is not "less information",
    /// it is the *wrong answer* to `decls`, and an implementation that acts on
    /// it builds a tree from a class it half understands.
    ///
    /// The refusals, each of which leaves the old empty placeholder:
    ///
    /// * a class with type parameters -- the engine is given a `typeRef` with
    ///   no arguments, so its type parameters would have nothing to bind;
    /// * a member scala-rs cannot describe: a nested class or type member, a
    ///   polymorphic method, a parameter or result type
    ///   [`Typer::type_to_wire`] refuses;
    /// * **a class already being described**, which is the cycle case. A
    ///   parent or a member type that names the class whose description is
    ///   being built would recurse for ever. nsc says `illegal cyclic
    ///   reference` here; this refuses and the class stays a placeholder,
    ///   which is the same answer one level out.
    fn run_class_wire(&mut self, sym: SymbolId, full: &str) -> String {
        match self.describe_run_class(sym, full) {
            Ok(desc) => desc,
            Err(why) => {
                // Remembered so that if the implementation then trips over the
                // empty placeholder -- which it does the moment it asks the
                // symbol anything, `tpe.toString` included -- the diagnostic
                // says *why the mirror had no answer* rather than repeating
                // an `AssertionError` out of the reflect internals.
                self.macro_undescribed.push((full.to_string(), why));
                let mut out = String::from("(syn ");
                quote_into(&mut out, full);
                out.push(')');
                out
            }
        }
    }

    fn describe_run_class(&mut self, sym: SymbolId, full: &str) -> Result<String, String> {
        if self.macro_rpc_forcing.iter().any(|n| n == full) {
            return Err(format!("`{full}` is already being described"));
        }
        if !self.st.get(sym).tparams.is_empty() {
            return Err(format!("`{full}` has type parameters"));
        }
        self.macro_rpc_forcing.push(full.to_string());
        let built = self.describe_run_class_body(sym);
        self.macro_rpc_forcing.pop();
        let (parents, decls) = built?;
        let mut out = String::from("(run ");
        quote_into(&mut out, full);
        out.push_str(&class_flags_wire(self.st.get(sym).flags));
        out.push_str(" (parents");
        for p in parents {
            out.push(' ');
            out.push_str(&p);
        }
        out.push_str(") (decls");
        for d in decls {
            out.push(' ');
            out.push_str(&d);
        }
        out.push_str("))");
        Ok(out)
    }

    fn describe_run_class_body(
        &mut self,
        sym: SymbolId,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let mut parents = Vec::new();
        for p in self.st.get(sym).parents.clone() {
            // `scala.AnyRef` is `java.lang.Object` -- 2.13 declares it as that
            // alias -- and the engine's mirror has no `staticClass` for the
            // alias, only for the class it names. Written out here rather than
            // in `type_to_wire`, which answers *what a tree's type is* and
            // must keep saying `AnyRef` when that is what the typer said.
            let p = if matches!(p, Type::AnyRef) {
                "(ty \"java.lang.Object\")".to_string()
            } else {
                self.type_to_wire(&p)?
            };
            parents.push(p);
        }
        if parents.is_empty() {
            parents.push("(ty \"java.lang.Object\")".to_string());
        }
        let mut decls = Vec::new();
        for m in self.st.get(sym).members.clone() {
            decls.push(self.describe_run_member(m)?);
        }
        Ok((parents, decls))
    }

    /// One declared member, or why it cannot be described.
    fn describe_run_member(&mut self, m: SymbolId) -> Result<String, String> {
        let (kind, name, flags, tparams, paramss, ty) = {
            let s = self.st.get(m);
            (
                s.kind,
                s.name.clone(),
                s.flags,
                s.tparams.clone(),
                s.paramss.clone(),
                s.ty.clone(),
            )
        };
        match kind {
            SymKind::Method => {}
            // A `val` is one symbol here and *two* in nsc -- a private field
            // and a STABLE accessor, both in `decls`. Describing it as either
            // one would be describing a different class, so a class with a
            // field is not described at all and stays the empty placeholder.
            SymKind::Term => {
                return Err(format!(
                    "`{name}` is a field, and scala-rs models a `val` as one symbol \
                     where nsc has a private field and a stable accessor"
                ))
            }
            _ => {
                return Err(format!(
                    "`{name}` is a {kind:?}, which scala-rs cannot describe to the engine"
                ))
            }
        }
        if !tparams.is_empty() {
            return Err(format!("`{name}` is polymorphic"));
        }
        if paramss.len() > 1 {
            return Err(format!("`{name}` has more than one parameter clause"));
        }
        let (params, result) = match &ty {
            Type::Method { paramss: ps, ret } if ps.len() <= 1 => {
                (ps.first().cloned().unwrap_or_default(), (**ret).clone())
            }
            Type::Method { .. } => {
                return Err(format!("`{name}` has more than one parameter clause"))
            }
            other => (Vec::new(), other.clone()),
        };
        // `def f()` and `def f` are different members in nsc; the empty
        // clause has to survive, so it is written as its own marker.
        let empty_clause = matches!(&ty, Type::Method { paramss: ps, .. }
            if ps.len() == 1 && ps[0].is_empty());
        let mut written = Vec::new();
        for p in &params {
            written.push(self.type_to_wire(p)?);
        }
        let result = self.type_to_wire(&result)?;
        // The primary constructor. Its result type is the class itself in
        // nsc and `Unit` here, and its empty clause is real, so it travels as
        // a marker and the engine fills the result in.
        let ctor = name == "<init>";
        let mut out = String::from("(d ");
        quote_into(&mut out, &encode_method_name(&name));
        out.push_str(&member_flags_wire(flags, kind, ctor));
        if ctor {
            out.push_str(" (params");
            for w in &written {
                out.push(' ');
                out.push_str(w);
            }
            out.push(')');
            out.push_str(" (ty \"scala.Unit\"))");
            return Ok(out);
        }
        if params.is_empty() && !empty_clause {
            out.push_str(" (nullary)");
        } else {
            out.push_str(" (params");
            for w in written {
                out.push(' ');
                out.push_str(&w);
            }
            out.push(')');
        }
        out.push(' ');
        out.push_str(&result);
        out.push(')');
        Ok(out)
    }

    /// Whether `sym` is a class this compilation run is itself defining --
    /// that is, one with no class file for the engine's mirror to find. The
    /// same test [`Typer::tag_descriptor`] makes, and for the same reason.
    fn is_current_run_class(&mut self, sym: SymbolId) -> bool {
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
fn answer_tree_to_wire(st: &SymbolTable, t: &Tree, out: &mut String) -> Result<(), String> {
    let unsupported = |what: &str| Err(format!("{what}, which scala-rs cannot write back"));
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
            match owner_qualifier(st, t.sym) {
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
            answer_tree_to_wire(st, qual, out)?;
            out.push_str(" (n term ");
            quote_into(out, &encode_method_name(name));
            out.push_str("))");
            Ok(())
        }
        TreeKind::Apply { fun, args } => {
            out.push_str("(t \"Apply\" (s0) ");
            answer_tree_to_wire(st, fun, out)?;
            out.push_str(" (l");
            for a in args {
                out.push(' ');
                answer_tree_to_wire(st, a, out)?;
            }
            out.push_str("))");
            Ok(())
        }
        TreeKind::Block { stats, expr } => {
            out.push_str("(t \"Block\" (s0) (l");
            for s in stats {
                out.push(' ');
                answer_tree_to_wire(st, s, out)?;
            }
            out.push_str(") ");
            answer_tree_to_wire(st, expr, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::If { cond, thenp, elsep } => {
            out.push_str("(t \"If\" (s0) ");
            answer_tree_to_wire(st, cond, out)?;
            out.push(' ');
            answer_tree_to_wire(st, thenp, out)?;
            out.push(' ');
            answer_tree_to_wire(st, elsep, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Empty => {
            out.push_str("(t \"EmptyTree\" (s0))");
            Ok(())
        }
        TreeKind::Typed { .. } => unsupported("a type ascription"),
        TreeKind::Function { .. } => unsupported("a function literal"),
        TreeKind::New { .. } => unsupported("a `new`"),
        TreeKind::Match { .. } => unsupported("a `match`"),
        TreeKind::TypeApply { .. } => unsupported("an explicit type application"),
        TreeKind::ValDef { .. } => unsupported("a `val` definition"),
        _ => unsupported("a tree of this form"),
    }
}

/// The `C` of the `C.this` a typed `Ident` naming a member of `C` stands for.
///
/// Only a *class* owner: a name owned by a method is a local, and a local has
/// no path at all -- writing `C.this.x` for one would be a different tree.
fn owner_qualifier(st: &SymbolTable, sym: SymbolId) -> Option<String> {
    if sym == SymbolId::NONE {
        return None;
    }
    let owner = st.get(sym).owner;
    if owner == SymbolId::NONE {
        return None;
    }
    let name = &st.get(owner).name;
    match st.get(owner).kind {
        SymKind::Class => Some(name.clone()),
        SymKind::ModuleClass => Some(name.strip_suffix('$').unwrap_or(name).to_string()),
        _ => None,
    }
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

/// Why an implementation's own failure may be a consequence of a class the
/// mirror could not describe.
///
/// A class this run is compiling reaches the engine either fully described
/// ([`Typer::run_class_wire`]) or as the empty placeholder of
/// `docs/macros.md` §5.1. In the second case *any* question about it throws
/// out of the reflect internals -- `assertion failed: <name>` from
/// `Symbol.info`, which is not a sentence about the program being compiled.
/// This replaces it with the reason scala-rs had, which is.
pub(crate) fn undescribed_verdict(undescribed: &[(String, String)], msg: &str) -> Option<String> {
    if undescribed.is_empty() {
        return None;
    }
    let names = undescribed
        .iter()
        .map(|(n, why)| format!("`{n}` ({why})"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "the implementation asked about {names}, and scala-rs could not \
         describe {} to the macro engine, so the engine was given a symbol \
         carrying only a name; asking it anything raised \"{msg}\", which says \
         nothing about this program",
        if undescribed.len() == 1 { "it" } else { "them" }
    ))
}
