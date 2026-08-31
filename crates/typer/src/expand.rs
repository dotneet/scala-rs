//! Def-macro expansion: the JVM bridge (`docs/macros.md` §2.2, phase 2).
//!
//! nsc expands a macro by *running* its implementation: it loads the
//! implementation class from the macro classpath and calls it through Java
//! reflection with a `scala.reflect.macros.blackbox.Context`, then typechecks
//! the tree that comes back at the call site. scala-rs is not on the JVM, so
//! the running half lives in a small Java program
//! (`crates/typer/java/ScalaRsMacroEngine.java`, embedded below) that this
//! module starts once per run and talks to over a pipe.
//!
//! ```text
//! scala-rs (Rust)                        engine (JVM)
//! ───────────────                        ────────────
//! outermost macro application
//!   argument trees + type arguments
//!                          ──────→       Context proxy, universe =
//!                                          scala.reflect.runtime.universe
//!                                        build the argument Exprs and tags
//!                                        invoke the implementation
//!                          ←──────       the returned Tree, written back
//! rebuild it as an untyped tree
//! typecheck it at the call site
//! ```
//!
//! **The subset is deliberate and every gap is a diagnostic.** An argument
//! shape this module cannot hand over, a node kind it cannot rebuild, a
//! missing `java`, a missing scala-reflect.jar: each of those ends the
//! expansion with a *reason*, which [`Typer::report_macro_calls`] prints
//! attached to the same "macro expansion is not implemented" error the call
//! site got before this module existed. A macro is never quietly accepted --
//! the macro def has no bytecode, so accepting one would emit a call to a
//! method that is not there -- and never quietly expanded to something other
//! than what the implementation returned.
//!
//! What works today, and what does not, is in `docs/macros.md` §7.11.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use scala_rs_parser::{Lit, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_pickle::names::{decode_method_name, encode_method_name};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::MacroBinding;

/// The engine's source. Written to a cache directory and compiled with
/// `javac` on first use, so the repository carries no class files and the
/// build needs no JVM.
const ENGINE_SOURCE: &str = include_str!("../java/ScalaRsMacroEngine.java");

/// nsc's `-Ymacro-expand-depth`. A macro whose expansion calls itself has to
/// stop somewhere, and stopping with a diagnostic beats a stack overflow.
const MAX_EXPANSION_DEPTH: u32 = 32;

// ---------------------------------------------------------------- the process

/// The engine process, started on the first expansion of a run.
pub(crate) struct MacroEngine {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for MacroEngine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl MacroEngine {
    /// One request, one reply. `Err` is a reason, already phrased for a user.
    fn ask(&mut self, request: &str) -> Result<Sexp, String> {
        writeln!(self.stdin, "{request}").map_err(|e| format!("the macro engine died ({e})"))?;
        self.stdin
            .flush()
            .map_err(|e| format!("the macro engine died ({e})"))?;
        let mut line = String::new();
        match self.stdout.read_line(&mut line) {
            Ok(0) => Err("the macro engine exited without a reply".to_string()),
            Ok(_) => Sexp::parse(line.trim_end()),
            Err(e) => Err(format!("the macro engine died ({e})")),
        }
    }
}

/// Compile the engine into a cache directory and start it.
///
/// The classpath handed to `java` is the compilation's own binary path: the
/// macro implementation's class files, scala-library.jar and
/// scala-reflect.jar. nsc uses the compilation classpath for exactly the same
/// reason, and `reify`'s `mirror.staticModule` needs the *compiled program's*
/// classes on it too, not only the implementation's.
fn start_engine(classpath: &[PathBuf]) -> Result<MacroEngine, String> {
    if !classpath.iter().any(|p| is_scala_reflect(p)) {
        return Err("scala-reflect.jar is not on the classpath, and a macro \
                    implementation cannot be run without it"
            .to_string());
    }
    let dir = engine_dir();
    let class_file = dir.join("ScalaRsMacroEngine.class");
    if !class_file.is_file() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create the macro engine directory: {e}"))?;
        let src = dir.join("ScalaRsMacroEngine.java");
        std::fs::write(&src, ENGINE_SOURCE)
            .map_err(|e| format!("cannot write the macro engine source: {e}"))?;
        let out = Command::new("javac")
            .arg("-d")
            .arg(&dir)
            .arg(&src)
            .output()
            .map_err(|e| format!("cannot run `javac` to build the macro engine: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "the macro engine does not compile: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut cp = dir.display().to_string();
    for p in classpath {
        cp.push(sep);
        cp.push_str(&p.display().to_string());
    }
    let mut child = Command::new("java")
        .arg("-cp")
        .arg(&cp)
        .arg("ScalaRsMacroEngine")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start `java` to expand macros: {e}"))?;
    let stdin = child.stdin.take().expect("piped stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut hello = String::new();
    match stdout.read_line(&mut hello) {
        Ok(0) => return Err("the macro engine exited at startup".to_string()),
        Err(e) => return Err(format!("the macro engine died at startup ({e})")),
        Ok(_) => {}
    }
    let engine = MacroEngine {
        child,
        stdin,
        stdout,
    };
    if hello.trim_end() != "(ready)" {
        let why = match Sexp::parse(hello.trim_end()) {
            Ok(s) => s.reason().unwrap_or_else(|| hello.trim_end().to_string()),
            Err(_) => hello.trim_end().to_string(),
        };
        // Dropped here so the failed child does not outlive the diagnostic.
        drop(engine);
        return Err(why);
    }
    Ok(engine)
}

fn is_scala_reflect(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("scala-reflect"))
}

/// Where the compiled engine is cached, keyed by the source it was built from
/// so an updated engine is never run from a stale class file.
fn engine_dir() -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ENGINE_SOURCE.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    std::env::temp_dir().join(format!("scala-rs-macro-engine-{h:016x}"))
}

// -------------------------------------------------------------- the expansion

impl Typer {
    /// Expand `tree` if it is a macro application, in place.
    ///
    /// Called from [`Typer::type_expr`] at the outermost node of an
    /// application, which is where nsc expands. Doing nothing is always safe:
    /// `report_macro_calls` sweeps the typed tree afterwards and turns every
    /// macro application still standing into an error.
    pub(crate) fn expand_macro_application(&mut self, tree: &mut Tree) {
        if self.sigs_only {
            return;
        }
        let Some(sym) = self.macro_symbol_of(tree) else {
            return;
        };
        // Not applied yet: the inner `Apply` of a curried macro still has a
        // method type, and so does a macro def named but not called.
        if matches!(tree.ty, Type::Method { .. })
            || tree.ty.is_error()
            || tree.ty.is_no_type()
            || matches!(tree.ty, Type::Overload(_))
        {
            return;
        }
        let binding = match self.st.get(sym).macro_impl.clone() {
            Some(b) => b,
            None => return,
        };
        if self.macro_depth >= MAX_EXPANSION_DEPTH {
            self.note_macro_failure(
                tree.span,
                format!("expansion recursed more than {MAX_EXPANSION_DEPTH} deep"),
            );
            return;
        }
        match self.macro_expansion(tree, &binding) {
            Ok(mut built) => {
                let declared = tree.ty.clone();
                built.span = tree.span;
                *tree = built;
                self.macro_depth += 1;
                // A blackbox macro's expansion is typechecked *against the
                // declared result type* and keeps it, whatever more precise
                // type the expansion itself has (nsc ascribes the expansion
                // with `Typed(expanded, TypeTree(innerPt))`).
                self.type_expr(tree, &declared);
                self.macro_depth -= 1;
                if !tree.ty.is_error() {
                    tree.ty = declared;
                }
            }
            Err(reason) => self.note_macro_failure(tree.span, reason),
        }
    }

    /// Run the implementation and rebuild what it returned.
    fn macro_expansion(&mut self, tree: &Tree, binding: &MacroBinding) -> Result<Tree, String> {
        let (argss, targs) = peel_application(tree);
        let request = self.expansion_request(binding, &argss, &targs)?;
        if let Some(why) = &self.macro_engine_error {
            // Starting it costs a `javac` and a JVM; a run whose first attempt
            // failed must not pay that again at every call site.
            return Err(why.clone());
        }
        if self.macro_engine.is_none() {
            let cp = self.macro_classpath.clone();
            match start_engine(&cp) {
                Ok(e) => self.macro_engine = Some(e),
                Err(why) => {
                    self.macro_engine_error = Some(why.clone());
                    return Err(why);
                }
            }
        }
        let reply = self
            .macro_engine
            .as_mut()
            .expect("engine started")
            .ask(&request)?;
        let items = reply.list()?;
        match items.first().and_then(|s| s.atom()) {
            Some("ok") => self.tree_from_reply(at(items, 1)?, tree.span),
            Some("abort") => {
                // `c.abort` is the implementation asking for a compile error
                // at the call site. It is not a gap in this expander, so it is
                // reported as itself.
                let msg = at(items, 1)?.text();
                self.error(tree.span, msg);
                Err("the macro implementation aborted the expansion".to_string())
            }
            Some("err") => Err(at(items, 1)?.text()),
            _ => Err(format!("the macro engine replied {reply:?}")),
        }
    }

    /// Serialise one expansion request.
    fn expansion_request(
        &mut self,
        binding: &MacroBinding,
        argss: &[Vec<Tree>],
        targs: &[Type],
    ) -> Result<String, String> {
        let mut out = String::from("(expand ");
        quote_into(&mut out, &binding.impl_class);
        out.push(' ');
        quote_into(&mut out, &binding.impl_method);
        let supplied: usize = argss.iter().map(|c| c.len()).sum();
        if supplied != binding.expr_args.len() {
            return Err(format!(
                "the implementation takes {} argument(s) and the call site supplies {supplied}",
                binding.expr_args.len()
            ));
        }
        out.push_str(" (argss");
        let mut at = 0;
        for clause in argss {
            out.push_str(" (args");
            for a in clause {
                out.push_str(if binding.expr_args[at] {
                    " (arg expr "
                } else {
                    " (arg tree "
                });
                let as_expr = binding.expr_args[at];
                at += 1;
                tree_to_wire(a, &mut out)?;
                out.push(' ');
                if as_expr {
                    // Only an `Expr` carries a tag, and only the tag needs a
                    // type the engine can rebuild.
                    type_to_wire(&self.st, &a.ty, &mut out)?;
                } else {
                    out.push_str("(ty \"\")");
                }
                out.push(')');
            }
            out.push(')');
        }
        out.push_str(") (tags");
        if binding.tag_params > 0 {
            if targs.len() != binding.tag_params {
                return Err(format!(
                    "the implementation asks for {} type tag(s) and the call site \
                     supplies {} type argument(s); an inferred type argument is not \
                     passed to a macro yet",
                    binding.tag_params,
                    targs.len()
                ));
            }
            for t in targs {
                out.push(' ');
                type_to_wire(&self.st, t, &mut out)?;
            }
        }
        out.push_str("))");
        Ok(out)
    }

    /// Remember why one call site could not be expanded, so
    /// `report_macro_calls` can name it.
    fn note_macro_failure(&mut self, span: Span, reason: String) {
        let key = self.macro_failure_key(span);
        self.macro_failures.insert(key, reason);
    }

    /// A call site, identified across the whole run. Positions are per file,
    /// so the file has to be part of the key.
    pub(crate) fn macro_failure_key(&self, span: Span) -> (usize, u32, u32) {
        (
            self.file_index,
            span.lo.to_usize() as u32,
            span.hi.to_usize() as u32,
        )
    }

    // ------------------------------------------------------ reply → our tree

    /// Rebuild the reflect tree the engine wrote as an *untyped* scala-rs
    /// tree, ready to be typechecked at the call site.
    fn tree_from_reply(&mut self, s: &Sexp, span: Span) -> Result<Tree, String> {
        let items = s.list()?;
        if items.first().and_then(|s| s.atom()) != Some("t") {
            return Err(format!("the macro engine returned {s:?}"));
        }
        let kind = at(items, 1)?.text();
        let sym = at(items, 2)?.list()?;
        let full = if sym.first().and_then(|s| s.atom()) == Some("s") {
            Some(at(sym, 1)?.text())
        } else {
            None
        };
        let kids = items.get(3..).unwrap_or(&[]);
        let node = |kind| Tree {
            id: NodeId(0),
            span,
            kind,
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        match kind.as_str() {
            "Literal" => Ok(node(TreeKind::Literal {
                lit: literal_from(at(kids, 0)?)?,
            })),
            "Ident" => {
                let name = decode_method_name(&name_from(at(kids, 0)?)?);
                // A *static* symbol is rebuilt from its full name: the
                // expansion is typed in the call site's scope, where the
                // implementation's own imports do not exist, so `Ident(Helper)`
                // has to become the path `Helper` really names.
                match full {
                    Some(f) if f.contains('.') => Ok(path_tree(&f, span)),
                    _ => Ok(node(TreeKind::Ident { name })),
                }
            }
            "Select" => {
                let qual = self.tree_from_reply(at(kids, 0)?, span)?;
                let name = decode_method_name(&name_from(at(kids, 1)?)?);
                Ok(node(TreeKind::Select {
                    qual: Box::new(qual),
                    name,
                }))
            }
            "Apply" => {
                let fun = self.tree_from_reply(at(kids, 0)?, span)?;
                let mut args = Vec::new();
                for a in at(kids, 1)?.list()?.iter().skip(1) {
                    args.push(self.tree_from_reply(a, span)?);
                }
                Ok(node(TreeKind::Apply {
                    fun: Box::new(fun),
                    args,
                }))
            }
            "TypeApply" => {
                let fun = self.tree_from_reply(at(kids, 0)?, span)?;
                let mut args = Vec::new();
                for a in at(kids, 1)?.list()?.iter().skip(1) {
                    args.push(self.tree_from_reply(a, span)?);
                }
                Ok(node(TreeKind::TypeApply {
                    fun: Box::new(fun),
                    args,
                }))
            }
            "Block" => {
                let mut stats = Vec::new();
                for a in at(kids, 0)?.list()?.iter().skip(1) {
                    stats.push(self.tree_from_reply(a, span)?);
                }
                let expr = self.tree_from_reply(at(kids, 1)?, span)?;
                Ok(node(TreeKind::Block {
                    stats,
                    expr: Box::new(expr),
                }))
            }
            "If" => {
                let cond = self.tree_from_reply(at(kids, 0)?, span)?;
                let thenp = self.tree_from_reply(at(kids, 1)?, span)?;
                let elsep = self.tree_from_reply(at(kids, 2)?, span)?;
                Ok(node(TreeKind::If {
                    cond: Box::new(cond),
                    thenp: Box::new(thenp),
                    elsep: Box::new(elsep),
                }))
            }
            "Typed" => {
                let expr = self.tree_from_reply(at(kids, 0)?, span)?;
                let tpt = self.tree_from_reply(at(kids, 1)?, span)?;
                Ok(node(TreeKind::Typed {
                    expr: Box::new(expr),
                    tpt: Box::new(tpt),
                }))
            }
            "TypeTree" => {
                let items = at(kids, 0)?.list()?;
                let name = at(items, 1)?.text();
                if items.len() > 2 {
                    return Err(format!(
                        "the expansion mentions the type `{name}`, whose type \
                         arguments scala-rs cannot rebuild yet"
                    ));
                }
                if name.is_empty() {
                    return Err("the expansion contains an empty `TypeTree`".to_string());
                }
                // A type path is the same tree shape as a term path here, and
                // `tree_to_type` reads it.
                Ok(path_tree(&name, span))
            }
            "This" => Ok(node(TreeKind::This { qual: None })),
            "EmptyTree" => Ok(node(TreeKind::Empty)),
            other => Err(format!(
                "the expansion contains a `{other}`, which scala-rs cannot rebuild yet"
            )),
        }
    }
}

/// `a.b.C` as a term path.
fn path_tree(full: &str, span: Span) -> Tree {
    let mut parts = full.split('.');
    let head = parts.next().unwrap_or("");
    let mut t = Tree {
        id: NodeId(0),
        span,
        kind: TreeKind::Ident {
            name: head.to_string(),
        },
        ty: Type::NoType,
        sym: SymbolId::NONE,
        postfix: false,
    };
    for p in parts {
        t = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(t),
                name: p.to_string(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
    }
    t
}

fn literal_from(s: &Sexp) -> Result<Lit, String> {
    let items = s.list()?;
    if items.first().and_then(|s| s.atom()) != Some("c") {
        return Err(format!("expected a constant, got {s:?}"));
    }
    let kind = at(items, 1)?.text();
    let text = at(items, 2)?.text();
    let bad = |what: &str| format!("the expansion contains a malformed {what} constant");
    match kind.as_str() {
        "Unit" => Ok(Lit::Unit),
        "Null" => Ok(Lit::Null),
        "Boolean" => Ok(Lit::Boolean(text == "true")),
        "Char" => text
            .chars()
            .next()
            .map(Lit::Char)
            .ok_or_else(|| bad("Char")),
        "Int" => text.parse().map(Lit::Int).map_err(|_| bad("Int")),
        "Long" => text.parse().map(Lit::Long).map_err(|_| bad("Long")),
        "Float" => text.parse().map(Lit::Float).map_err(|_| bad("Float")),
        "Double" => text.parse().map(Lit::Double).map_err(|_| bad("Double")),
        "String" => Ok(Lit::String(text)),
        other => Err(format!(
            "the expansion contains a `{other}` constant, which scala-rs cannot rebuild yet"
        )),
    }
}

/// The `i`th item of a reply node. The engine is a separate process, so a
/// short node is a protocol error to report, never a panic in the compiler.
fn at(items: &[Sexp], i: usize) -> Result<&Sexp, String> {
    items
        .get(i)
        .ok_or_else(|| "the macro engine sent a truncated node".to_string())
}

fn name_from(s: &Sexp) -> Result<String, String> {
    let items = s.list()?;
    match items.first().and_then(|s| s.atom()) {
        Some("n") => Ok(at(items, 2)?.text()),
        _ => Err(format!("expected a name, got {s:?}")),
    }
}

/// The argument clauses and explicit type arguments of a macro application,
/// outermost application last -- i.e. in source order.
fn peel_application(tree: &Tree) -> (Vec<Vec<Tree>>, Vec<Type>) {
    let mut argss: Vec<Vec<Tree>> = Vec::new();
    let mut targs: Vec<Type> = Vec::new();
    let mut t = tree;
    loop {
        match &t.kind {
            TreeKind::Apply { fun, args } => {
                argss.insert(0, args.clone());
                t = fun;
            }
            TreeKind::TypeApply { fun, args } => {
                targs = args.iter().map(|a| a.ty.clone()).collect();
                t = fun;
            }
            _ => break,
        }
    }
    (argss, targs)
}

// ------------------------------------------------------------ our tree → wire

/// Write an argument tree in the shape the engine can rebuild.
///
/// Only the forms whose *source* meaning survives being rebuilt at the call
/// site are sent. The expansion is typechecked again where the macro was
/// called, so anything the typer has already rewritten (an inserted implicit
/// conversion, a desugared for-comprehension) would be typed a second time;
/// refusing those by name is the honest answer until the bridge carries typed
/// trees (`docs/macros.md` §4.3).
fn tree_to_wire(t: &Tree, out: &mut String) -> Result<(), String> {
    let unsupported = |what: &str| {
        Err(format!(
            "scala-rs cannot hand {what} to a macro implementation yet"
        ))
    };
    match &t.kind {
        TreeKind::Literal { lit } => {
            out.push_str("(t \"Literal\" (s0) ");
            lit_to_wire(lit, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Ident { name } => {
            out.push_str("(t \"Ident\" (s0) (n term ");
            // Reflect names are NameTransformer-encoded (`+` is `$plus`), the
            // way nsc hands them to a macro.
            quote_into(out, &encode_method_name(name));
            out.push_str("))");
            Ok(())
        }
        TreeKind::This { .. } => {
            out.push_str("(t \"This\" (s0) (n type \"\"))");
            Ok(())
        }
        TreeKind::Select { qual, name } => {
            out.push_str("(t \"Select\" (s0) ");
            tree_to_wire(qual, out)?;
            out.push_str(" (n term ");
            quote_into(out, &encode_method_name(name));
            out.push_str("))");
            Ok(())
        }
        TreeKind::Apply { fun, args } => {
            out.push_str("(t \"Apply\" (s0) ");
            tree_to_wire(fun, out)?;
            out.push_str(" (l");
            for a in args {
                out.push(' ');
                tree_to_wire(a, out)?;
            }
            out.push_str("))");
            Ok(())
        }
        TreeKind::Block { .. } => unsupported("a block"),
        TreeKind::Function { .. } => unsupported("a function literal"),
        TreeKind::New { .. } => unsupported("a `new`"),
        TreeKind::If { .. } => unsupported("an `if`"),
        TreeKind::Match { .. } => unsupported("a `match`"),
        TreeKind::TypeApply { .. } => unsupported("an explicit type application"),
        _ => unsupported("an argument of this form"),
    }
}

fn lit_to_wire(lit: &Lit, out: &mut String) -> Result<(), String> {
    let (kind, text) = match lit {
        Lit::Unit => ("Unit", "()".to_string()),
        Lit::Null => ("Null", "null".to_string()),
        Lit::Boolean(b) => ("Boolean", b.to_string()),
        Lit::Char(c) => ("Char", c.to_string()),
        Lit::Int(n) => ("Int", n.to_string()),
        Lit::Long(n) => ("Long", n.to_string()),
        Lit::Float(n) => ("Float", n.to_string()),
        Lit::Double(n) => ("Double", n.to_string()),
        Lit::String(s) => ("String", s.clone()),
        Lit::Symbol(_) => {
            return Err("scala-rs cannot hand a `'symbol` literal to a macro \
                        implementation yet"
                .to_string())
        }
    };
    out.push_str("(c ");
    quote_into(out, kind);
    out.push(' ');
    quote_into(out, &text);
    out.push(')');
    Ok(())
}

/// A type the engine can rebuild with one `mirror.staticClass` call.
///
/// The same restriction as `TypeTag` materialisation (`docs/macros.md`
/// §7.10): a monomorphic class, named, and nothing else -- a wrong tag would
/// reach the implementation as a wrong `Type` and be discovered only as a
/// wrong answer at run time.
fn type_to_wire(
    st: &crate::symbol::SymbolTable,
    ty: &Type,
    out: &mut String,
) -> Result<(), String> {
    // `f(42)` types its argument as the *constant* type `42`; the tag nsc
    // builds for it is `Int`.
    let widened = match ty {
        Type::Constant(lit) => Type::lit_underlying(lit),
        other => other.clone(),
    };
    let name = crate::materialize::static_class_name(st, &widened)
        .map_err(|why| format!("scala-rs cannot build a type tag for {why}"))?;
    out.push_str("(ty ");
    quote_into(out, &name);
    out.push(')');
    Ok(())
}

// ------------------------------------------------------------------- the wire

/// The wire format: atoms, quoted strings and lists. Small enough to write
/// twice (here and in the engine) and to read in a debugger.
#[derive(Debug, Clone)]
pub(crate) enum Sexp {
    Atom(String),
    Str(String),
    List(Vec<Sexp>),
}

impl Sexp {
    fn parse(s: &str) -> Result<Sexp, String> {
        let bytes: Vec<char> = s.chars().collect();
        let mut i = 0;
        let v = Sexp::parse_at(&bytes, &mut i)?;
        Ok(v)
    }

    fn parse_at(s: &[char], i: &mut usize) -> Result<Sexp, String> {
        while *i < s.len() && s[*i] == ' ' {
            *i += 1;
        }
        if *i >= s.len() {
            return Err("the macro engine sent an empty reply".to_string());
        }
        match s[*i] {
            '(' => {
                *i += 1;
                let mut items = Vec::new();
                loop {
                    while *i < s.len() && s[*i] == ' ' {
                        *i += 1;
                    }
                    if *i >= s.len() {
                        return Err("the macro engine sent an unterminated reply".to_string());
                    }
                    if s[*i] == ')' {
                        *i += 1;
                        break;
                    }
                    items.push(Sexp::parse_at(s, i)?);
                }
                Ok(Sexp::List(items))
            }
            '"' => {
                *i += 1;
                let mut out = String::new();
                while *i < s.len() && s[*i] != '"' {
                    let c = s[*i];
                    *i += 1;
                    if c == '\\' && *i < s.len() {
                        let e = s[*i];
                        *i += 1;
                        out.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            other => other,
                        });
                    } else {
                        out.push(c);
                    }
                }
                *i += 1;
                Ok(Sexp::Str(out))
            }
            _ => {
                let mut out = String::new();
                while *i < s.len() && !matches!(s[*i], ' ' | '(' | ')') {
                    out.push(s[*i]);
                    *i += 1;
                }
                Ok(Sexp::Atom(out))
            }
        }
    }

    fn list(&self) -> Result<&Vec<Sexp>, String> {
        match self {
            Sexp::List(v) => Ok(v),
            other => Err(format!("the macro engine sent {other:?}")),
        }
    }

    fn atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(a) => Some(a),
            _ => None,
        }
    }

    /// The payload of an atom or string, whichever this is.
    fn text(&self) -> String {
        match self {
            Sexp::Atom(a) | Sexp::Str(a) => a.clone(),
            other => format!("{other:?}"),
        }
    }

    /// The message of an `(err "...")` reply.
    fn reason(&self) -> Option<String> {
        match self {
            Sexp::List(v) if v.len() == 2 && v[0].atom() == Some("err") => Some(v[1].text()),
            _ => None,
        }
    }
}

fn quote_into(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_reply() {
        let s = Sexp::parse(r#"(ok (t "Literal" (s0) (c "Int" "42")))"#).unwrap();
        let items = s.list().unwrap();
        assert_eq!(items[0].atom(), Some("ok"));
        let t = items[1].list().unwrap();
        assert_eq!(t[1].text(), "Literal");
        assert_eq!(t[3].list().unwrap()[2].text(), "42");
    }

    #[test]
    fn unescapes_strings() {
        let s = Sexp::parse(r#"(err "a \"b\" c\nd")"#).unwrap();
        assert_eq!(s.reason().unwrap(), "a \"b\" c\nd");
    }

    #[test]
    fn quotes_what_it_parses() {
        let mut out = String::new();
        quote_into(&mut out, "a\"b\\c\n");
        assert_eq!(Sexp::parse(&out).unwrap().text(), "a\"b\\c\n");
    }
}
