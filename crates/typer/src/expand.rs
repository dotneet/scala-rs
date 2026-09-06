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

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{atomic::AtomicBool, atomic::Ordering as AtomicOrdering, Arc, Mutex};
use std::time::{Duration, Instant};

use scala_rs_parser::{Flags, Lit, Modifiers, NodeId, SymbolId, Tree, TreeKind, Type};
use scala_rs_pickle::names::{decode_method_name, encode_method_name};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::{MacroBinding, SymKind, SymbolTable};

/// The engine's source. Written to a cache directory and compiled with
/// `javac` on first use, so the repository carries no class files and the
/// build needs no JVM.
const ENGINE_SOURCE: &str = include_str!("../java/ScalaRsMacroEngine.java");

/// Keep the bridge runnable by the oldest JVM commonly used with Scala 2.13.
const ENGINE_JAVA_RELEASE: &str = "8";

/// Bump this when the cache layout or compiler policy changes.  In
/// particular, this keeps class files written by the pre-Java-8-target engine
/// out of the new cache without deleting a shared cache directory.
const ENGINE_CACHE_VERSION: &str = "java8-target-v2";

const MAX_ENGINE_DIAGNOSTIC_BYTES: usize = 16 * 1024;
const ENGINE_STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const ENGINE_STDERR_DRAIN_TIMEOUT: Duration = Duration::from_millis(200);

/// nsc's `-Ymacro-expand-depth`. A macro whose expansion calls itself has to
/// stop somewhere, and stopping with a diagnostic beats a stack overflow.
const MAX_EXPANSION_DEPTH: u32 = 32;

// ---------------------------------------------------------------- the process

/// The engine process, started on the first expansion of a run.
pub(crate) struct MacroEngine {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_done: Arc<AtomicBool>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
    /// Set when an expansion timed out and the child was killed: the pipe is
    /// no longer in sync with the requests, so nothing more may be asked.
    poisoned: bool,
}

impl Drop for MacroEngine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Dropping the handle detaches the bounded collector.  A macro is
        // allowed to spawn a child process that inherits stderr; joining here
        // would make compiler shutdown wait forever for that unrelated child.
        let _ = self.stderr_thread.take();
    }
}

/// How long one expansion may take before the engine is presumed hung.
///
/// A macro implementation is *user code* running inside the engine: it can
/// loop forever, deadlock, or block on something that never arrives, and
/// `read_line` on the pipe would wait for all of it. That is not theoretical
/// -- a killed parent once left twelve `scala-rs` processes blocked here for
/// nine minutes each, and they went on holding a core apiece until they were
/// killed by hand. A compiler must fail with a diagnostic instead of hanging,
/// so the read runs on a helper thread and this is how long we wait for it.
/// Override with `SCALA_RS_MACRO_TIMEOUT_SECS` (0 disables, for debugging an
/// implementation under a JVM debugger).
fn expansion_timeout() -> Option<Duration> {
    match std::env::var("SCALA_RS_MACRO_TIMEOUT_SECS") {
        Ok(v) => match v.trim().parse::<u64>() {
            Ok(0) => None,
            Ok(n) => Some(Duration::from_secs(n)),
            Err(_) => Some(Duration::from_secs(20)),
        },
        Err(_) => Some(Duration::from_secs(20)),
    }
}

impl MacroEngine {
    /// One request, one reply. `Err` is a reason, already phrased for a user.
    ///
    /// The reply is read on a helper thread so a wedged implementation costs a
    /// diagnostic and a killed child, not a process that never returns. Once
    /// timed out the engine is poisoned: the pipe still holds whatever that
    /// expansion eventually writes, so every later request would read the
    /// wrong reply.
    fn ask(&mut self, request: &str) -> Result<Sexp, String> {
        if self.poisoned {
            return Err("the macro engine was shut down after an expansion \
                        timed out; later expansions in this run cannot be \
                        trusted and are not attempted"
                .to_string());
        }
        writeln!(self.stdin, "{request}").map_err(|e| format!("the macro engine died ({e})"))?;
        self.stdin
            .flush()
            .map_err(|e| format!("the macro engine died ({e})"))?;

        let Some(limit) = expansion_timeout() else {
            let mut line = String::new();
            return match self.stdout.read_line(&mut line) {
                Ok(0) => Err("the macro engine exited without a reply".to_string()),
                Ok(_) => Sexp::parse(line.trim_end()),
                Err(e) => Err(format!("the macro engine died ({e})")),
            };
        };

        // `read_line` cannot be interrupted, so it runs where it can be
        // abandoned. The reader owns the handle for the duration and gives it
        // back with the line; on a timeout it is dropped along with the child.
        let mut stdout = std::mem::replace(&mut self.stdout, BufReader::new(dead_pipe()));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let r = stdout.read_line(&mut line).map(|n| (n, line));
            let _ = tx.send((stdout, r));
        });
        match rx.recv_timeout(limit) {
            Ok((stdout, r)) => {
                self.stdout = stdout;
                match r {
                    Ok((0, _)) => Err("the macro engine exited without a reply".to_string()),
                    Ok((_, line)) => Sexp::parse(line.trim_end()),
                    Err(e) => Err(format!("the macro engine died ({e})")),
                }
            }
            Err(_) => {
                self.poisoned = true;
                let _ = self.child.kill();
                let _ = self.child.wait();
                Err(format!(
                    "the macro implementation did not return within {}s -- it \
                     is looping, deadlocked, or waiting on something that \
                     never arrives (set SCALA_RS_MACRO_TIMEOUT_SECS to change \
                     or 0 to disable)",
                    limit.as_secs()
                ))
            }
        }
    }
}

/// A closed pipe to hold `stdout`'s place while the reader thread has it.
/// Reading it yields EOF, which is the truth once the child has been killed.
fn dead_pipe() -> ChildStdout {
    // `Stdio::null()` cannot become a `ChildStdout`, so borrow one from a
    // process that exits immediately.
    let mut c = Command::new("true")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn placeholder");
    let out = c.stdout.take().expect("placeholder stdout");
    let _ = c.wait();
    out
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
    if !valid_engine_class(&class_file) {
        compile_engine(&dir)?;
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut cp = dir.display().to_string();
    for p in classpath {
        cp.push(sep);
        cp.push_str(&p.display().to_string());
    }
    let mut child = Command::new(jdk_tool("java"))
        .arg("-cp")
        .arg(&cp)
        .arg("ScalaRsMacroEngine")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start `java` to expand macros: {e}"))?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    let stderr = child.stderr.take().expect("piped stderr");
    let (stderr, stderr_done, stderr_thread) = collect_engine_stderr(stderr);
    let (stdout, hello_result) = read_engine_hello(stdout);
    let hello = match hello_result {
        Ok(hello) => hello,
        Err(reason) => {
            let status = stop_engine(&mut child, Some(stderr_thread));
            wait_for_stderr(&stderr_done);
            return Err(startup_failure(&reason, status, &stderr));
        }
    };
    let engine = MacroEngine {
        child,
        stdin,
        stdout,
        stderr,
        stderr_done,
        stderr_thread: Some(stderr_thread),
        poisoned: false,
    };
    if hello.trim_end() != "(ready)" {
        let why = match Sexp::parse(hello.trim_end()) {
            Ok(s) => s.reason().unwrap_or_else(|| hello.trim_end().to_string()),
            Err(_) => hello.trim_end().to_string(),
        };
        // Dropped here so the failed child does not outlive the diagnostic.
        let mut engine = engine;
        let status = stop_engine(&mut engine.child, engine.stderr_thread.take());
        wait_for_stderr(&engine.stderr_done);
        return Err(startup_failure(&why, status, &engine.stderr));
    }
    Ok(engine)
}

/// Resolve both JVM tools from the same `JAVA_HOME` when one is supplied.
/// Build processes can inherit a different PATH from their parent shell, so
/// invoking bare `java` and `javac` can otherwise mix two JDK installations.
fn jdk_tool(name: &str) -> PathBuf {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let candidate = PathBuf::from(home).join("bin").join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

/// Compile into a private staging directory and publish it with one rename.
/// No compiler can observe a directory before its class file is complete, and
/// concurrent compiler processes can safely use the same cache key.
fn compile_engine(dir: &Path) -> Result<(), String> {
    static NEXT_STAGING_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT_STAGING_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let staging = dir.with_file_name(format!(
        "{}-staging-{}-{id}-{nanos}",
        dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("macro-engine"),
        std::process::id()
    ));
    std::fs::create_dir(&staging)
        .map_err(|e| format!("cannot create the macro engine staging directory: {e}"))?;
    let src = staging.join("ScalaRsMacroEngine.java");
    if let Err(e) = std::fs::write(&src, ENGINE_SOURCE) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("cannot write the macro engine source: {e}"));
    }

    let javac = jdk_tool("javac");
    let mut out = Command::new(&javac)
        .arg("--release")
        .arg(ENGINE_JAVA_RELEASE)
        .arg("-d")
        .arg(&staging)
        .arg(&src)
        .output()
        .map_err(|e| format!("cannot run `javac` to build the macro engine: {e}"))?;
    // JDK 8 predates --release.  The source is deliberately Java-8 API
    // compatible, so its default classfile target is correct there.
    if !out.status.success()
        && (out
            .stderr
            .windows(b"invalid flag:".len())
            .any(|w| w == b"invalid flag:")
            || out
                .stderr
                .windows(b"unrecognized option".len())
                .any(|w| w == b"unrecognized option"))
    {
        out = Command::new(&javac)
            .arg("-d")
            .arg(&staging)
            .arg(&src)
            .output()
            .map_err(|e| format!("cannot run `javac` to build the macro engine: {e}"))?;
    }
    if !out.status.success() {
        let detail = bounded_text(&out.stderr);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("the macro engine does not compile: {detail}"));
    }
    let staged_class = staging.join("ScalaRsMacroEngine.class");
    if !valid_engine_class(&staged_class) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("the macro engine compiler produced no valid class file".to_string());
    }
    match std::fs::rename(&staging, dir) {
        Ok(()) => Ok(()),
        Err(_e) if valid_engine_class(&dir.join("ScalaRsMacroEngine.class")) => {
            // Another process won the publication race.  Its complete cache
            // is authoritative; this process only cleans its own staging dir.
            let _ = std::fs::remove_dir_all(&staging);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(format!("cannot publish the macro engine cache: {e}"))
        }
    }
}

fn valid_engine_class(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.len() >= 8
        && bytes[0..4] == [0xca, 0xfe, 0xba, 0xbe]
        && u16::from_be_bytes([bytes[6], bytes[7]]) <= 52
}

fn collect_engine_stderr(
    stderr: ChildStderr,
) -> (
    Arc<Mutex<Vec<u8>>>,
    Arc<AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&captured);
    let done = Arc::new(AtomicBool::new(false));
    let thread_done = Arc::clone(&done);
    let thread = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut output) = shared.lock() {
                        append_bounded(&mut output, &buf[..n]);
                    }
                }
            }
        }
        thread_done.store(true, AtomicOrdering::Release);
    });
    (captured, done, thread)
}

/// Give the collector a short, bounded chance to observe EOF after the child
/// has exited.  Joining is unsafe here: a macro may leave a descendant holding
/// the inherited stderr pipe open indefinitely.
fn wait_for_stderr(done: &AtomicBool) {
    let deadline = Instant::now() + ENGINE_STDERR_DRAIN_TIMEOUT;
    while !done.load(AtomicOrdering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn append_bounded(output: &mut Vec<u8>, bytes: &[u8]) {
    let remaining = MAX_ENGINE_DIAGNOSTIC_BYTES.saturating_sub(output.len());
    output.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
}

fn bounded_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_ENGINE_DIAGNOSTIC_BYTES)])
        .trim()
        .to_string()
}

fn read_engine_hello(
    stdout: BufReader<ChildStdout>,
) -> (BufReader<ChildStdout>, Result<String, String>) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut hello = String::new();
        let result = stdout.read_line(&mut hello).map(|n| (n, hello));
        let _ = tx.send((stdout, result));
    });
    match rx.recv_timeout(ENGINE_STARTUP_TIMEOUT) {
        Ok((stdout, Ok((0, _)))) => (
            stdout,
            Err("the macro engine exited at startup".to_string()),
        ),
        Ok((stdout, Ok((_, hello)))) => (stdout, Ok(hello)),
        Ok((stdout, Err(e))) => (
            stdout,
            Err(format!("the macro engine died at startup ({e})")),
        ),
        Err(_) => (
            BufReader::new(dead_pipe()),
            Err(format!(
                "the macro engine did not report readiness within {}s",
                ENGINE_STARTUP_TIMEOUT.as_secs()
            )),
        ),
    }
}

fn stop_engine(
    child: &mut Child,
    _stderr_thread: Option<std::thread::JoinHandle<()>>,
) -> Option<std::process::ExitStatus> {
    let _ = child.kill();
    child.wait().ok()
}

fn startup_failure(
    reason: &str,
    status: Option<std::process::ExitStatus>,
    stderr: &Arc<Mutex<Vec<u8>>>,
) -> String {
    let mut message = reason.to_string();
    if let Some(status) = status {
        message.push_str(&format!(" (status: {status})"));
    }
    let detail = stderr
        .lock()
        .map(|bytes| bounded_text(&bytes))
        .unwrap_or_default();
    if !detail.is_empty() {
        message.push_str("; stderr: ");
        message.push_str(&detail);
    }
    message
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
    for b in ENGINE_CACHE_VERSION.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
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
    /// `Macros.foo _`: nsc's "macros cannot be eta-expanded".
    ///
    /// A macro def has no bytecode, so there is nothing for a method value to
    /// point at; nsc rejects the form outright rather than expanding the macro
    /// once and wrapping the expansion in a function. Reported here rather
    /// than left to [`Typer::report_macro_calls`], which would say "macro
    /// expansion is not implemented" -- true of nothing, since the expansion
    /// is not the problem.
    pub(crate) fn reject_macro_eta(&mut self, tree: &mut Tree) {
        if self.sigs_only || self.macro_symbol_of(tree).is_none() {
            return;
        }
        self.error(tree.span, "macros cannot be eta-expanded");
        tree.ty = Type::Error;
        // Cleared so the sweep does not report the same node a second time
        // with a reason that does not apply.
        tree.sym = SymbolId::NONE;
    }

    pub(crate) fn expand_macro_application(&mut self, tree: &mut Tree) {
        if self.sigs_only {
            return;
        }
        let Some(sym) = self.macro_symbol_of(tree) else {
            return;
        };
        // Not applied yet: the inner `Apply` of a curried macro still has a
        // method type, and so does a macro def named but not called.
        //
        // A *parameterless* macro def -- `def currentMirror: universe.Mirror
        // = macro ???` -- is the exception: it has no parameter clause to
        // supply, so the bare identifier already is the application, and its
        // type stays a `Method` with an empty `paramss` (`def f()`, which does
        // have a clause, is `[[]]` and is excluded here as before). Treating
        // it like a macro merely named left it unexpanded, and every use of
        // `scala.reflect.runtime.currentMirror` reported "cannot expand" with
        // no reason attached, because nothing had tried.
        let unapplied_clauses = match &tree.ty {
            Type::Method { paramss, .. } => !paramss.is_empty(),
            _ => false,
        };
        if unapplied_clauses
            || tree.ty.is_error()
            || tree.ty.is_no_type()
            || matches!(tree.ty, Type::Overload(_))
        {
            return;
        }
        // The macro application is the node that supplies the macro *def*'s
        // own parameter clauses, and no more. `M.f(1, 2)` where `f` takes
        // none -- a macro whose *result* is a function -- is an application
        // of the expansion, not of the macro: reading its argument list as
        // the macro's own reported "the implementation takes 0 argument(s)
        // and the call site supplies 2" for a call real scalac compiles.
        // Walk in to the node that does match, and expand there; the outer
        // application keeps the type it was already given, which is the
        // declared result type the expansion is checked against anyway.
        let want = match &self.st.get(sym).ty {
            Type::Method { paramss, .. } => paramss.len(),
            _ => 0,
        };
        if apply_layers(tree) > want {
            // The `apply` the typer inserted to call the expansion's result
            // sits between the two, so the node is found by matching rather
            // than by counting layers off the top.
            if let Some(inner) = macro_application_node(tree, sym, want) {
                self.expand_macro_application(inner);
                // The outer application still carries the macro's symbol from
                // when its callee was resolved. Left there,
                // `report_macro_calls` sees an unexpanded macro at a node that
                // is not one any more, and reports it with no reason at all.
                if tree.sym == sym {
                    tree.sym = SymbolId::NONE;
                }
            }
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
        // A macro nsc expands from its own `FastTrack` table rather than by
        // running an implementation (`crates/typer/src/fasttrack_mirror.rs`).
        // There is no bytecode to invoke for one, so this comes before the
        // request is even built.
        if let Some(built) = self.fasttrack_expansion(binding, tree.span) {
            return built;
        }
        let (argss, targs, prefix) = peel_application(tree);
        let (request, placeholders) =
            self.expansion_request(binding, &argss, &targs, prefix.as_ref(), tree)?;
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
                // reported as itself -- *unless* a placeholder went over, in
                // which case the implementation was answering a question about
                // a class it was never shown, and its verdict says nothing
                // about the program.
                let msg = at(items, 1)?.text();
                if let Some(why) = placeholder_verdict(&placeholders, &msg) {
                    return Err(why);
                }
                self.error(tree.span, msg);
                Err("the macro implementation aborted the expansion".to_string())
            }
            // An implementation that *threw* while a placeholder was in play
            // most likely asked it for the info it does not have; the engine
            // reports the exception, and the reason for it is said here.
            Some("err") => {
                let msg = at(items, 1)?.text();
                if msg.starts_with("the macro implementation threw") {
                    if let Some(why) = placeholder_verdict(&placeholders, &msg) {
                        return Err(why);
                    }
                }
                Err(msg)
            }
            _ => Err(format!("the macro engine replied {reply:?}")),
        }
    }

    /// Serialise one expansion request, and say which of its types went over
    /// as placeholders ([`Typer::tag_descriptor`]).
    fn expansion_request(
        &mut self,
        binding: &MacroBinding,
        argss: &[Vec<Tree>],
        targs: &[Type],
        prefix: Option<&Tree>,
        application: &Tree,
    ) -> Result<(String, Vec<String>), String> {
        let mut placeholders = Vec::new();
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
                    let desc = self.tag_descriptor(&a.ty, &mut placeholders)?;
                    out.push_str(&desc);
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
                let desc = self.tag_descriptor(t, &mut placeholders)?;
                out.push_str(&desc);
            }
        }
        out.push(')');
        // `c.prefix` -- the receiver the macro was called on. nsc hands the
        // implementation `Expr[Nothing](prefixTree)(TypeTag.Nothing)`, so only
        // the *tree* travels; the tag is a constant on the other side.
        //
        // Whether the implementation reads it is not knowable from here, so a
        // receiver this bridge cannot carry is *not* an error at the call
        // site: the reason travels instead and the engine raises it, named,
        // only if `prefix` is really asked for. Otherwise every macro called
        // on an awkward receiver would stop expanding for a member it never
        // touches.
        out.push_str(" (prefix ");
        match prefix {
            None => {
                out.push_str("(no ");
                quote_into(
                    &mut out,
                    "the macro was called without a receiver, and scala-rs does not \
                     synthesise the enclosing `this` for a prefix yet",
                );
                out.push(')');
            }
            Some(p) => {
                let mut built = String::new();
                match typed_tree_to_wire(&self.st, p, &mut built) {
                    Err(why) => {
                        out.push_str("(no ");
                        quote_into(&mut out, &why);
                        out.push(')');
                    }
                    Ok(()) => out.push_str(&built),
                }
            }
        }
        out.push(')');
        // `c.macroApplication` -- the whole call as written. nsc's own
        // `Position` travels with it there; here the tree is rebuilt in the
        // runtime universe and carries `NoPosition`, which costs nothing that
        // matters: an implementation uses it to place a diagnostic, and
        // scala-rs reports every diagnostic from a macro at the call site's
        // own span regardless. Carried the same way as `prefix`: a tree this
        // bridge cannot serialise is a named refusal when it is *asked for*,
        // not an error at every call site.
        out.push_str(" (app ");
        let mut built = String::new();
        match typed_tree_to_wire(&self.st, application, &mut built) {
            Err(why) => {
                out.push_str("(no ");
                quote_into(&mut out, &why);
                out.push(')');
            }
            Ok(()) => out.push_str(&built),
        }
        out.push(')');
        // `c.compilerSettings`. nsc hands the implementation the command line
        // that produced this run; a macro that gates on a flag (`scala.async`
        // on `-Xasync`) has no other way to see one.
        out.push_str(" (settings");
        for setting in &self.compiler_settings {
            out.push(' ');
            quote_into(&mut out, setting);
        }
        out.push_str("))");
        Ok((out, placeholders))
    }

    /// The wire descriptor for one type the engine has to turn into a tag.
    ///
    /// A class the engine's mirror can find on the macro classpath travels as
    /// its name, `(ty "a.b.C")`, and `mirror.staticClass` rebuilds it.
    ///
    /// A class **this run is compiling** has no class file for that mirror to
    /// find. slick's `TableQuery[Issues]` is exactly that shape -- the type
    /// argument is the table class declared a few lines from the call -- and
    /// it used to be refused outright. It now travels as `(syn "a.b.C")`, a
    /// *placeholder* symbol built in the runtime universe that carries the
    /// full name and no info at all, and the type it stands for is remembered
    /// here so that the expansion's own mention of it is read back as the
    /// type scala-rs already has rather than resolved again by name.
    ///
    /// The placeholder is deliberately empty. scala-rs cannot describe the
    /// class truthfully at this point in its own run: while
    /// `lazy val Issues = TableQuery[Issues]` is being typed, the members of
    /// `class Issues` are still un-inferred. An implementation that asks the
    /// placeholder a real question therefore gets an exception rather than a
    /// quiet wrong answer, and that becomes a diagnostic
    /// ([`Typer::macro_expansion`]).
    fn tag_descriptor(
        &mut self,
        ty: &Type,
        placeholders: &mut Vec<String>,
    ) -> Result<String, String> {
        if let Some(sym) = plain_class_of(&self.st, ty) {
            let jvm = self.st.jvm_internal(sym);
            if !jvm.is_empty() && !matches!(self.binary.find_class(&jvm), Ok(Some(_))) {
                let full = scala_full_name(&self.st, sym);
                self.macro_local_tags.insert(full.clone(), ty.clone());
                placeholders.push(full.clone());
                let mut out = String::from("(syn ");
                quote_into(&mut out, &full);
                out.push(')');
                return Ok(out);
            }
        }
        let name = static_tag_class(&self.st, ty)?;
        let mut out = String::from("(ty ");
        quote_into(&mut out, &name);
        out.push(')');
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
            scala_ref: false,
            stable_pat: false,
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
                // `new C(args)` is `Apply(Select(New(tpt), <init>), args)` in
                // reflect and `Apply(New(tpt), args)` here: the constructor
                // selection is spelled out there and implicit in our tree.
                if name == "<init>" && matches!(qual.kind, TreeKind::New { .. }) {
                    return Ok(qual);
                }
                Ok(node(TreeKind::Select {
                    qual: Box::new(qual),
                    name,
                }))
            }
            // `New(tpt)` on its own is `new C` with no argument list; nsc
            // always wraps it in the `<init>` selection above when there is
            // one, so both arrive here as our `New`.
            "New" => {
                let tpt = self.tree_from_reply(at(kids, 0)?, span)?;
                Ok(node(TreeKind::New { tpt: Box::new(tpt) }))
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
                // A class this run is compiling went over as a placeholder
                // carrying only its name (`tag_descriptor`). Coming back it is
                // that same name, and the type it stands for is the one the
                // typer already had -- resolving the name again would look for
                // a path that need not exist at the call site, because a class
                // nested in a trait has no such path at all.
                if let Some(ty) = self.macro_local_tags.get(&name) {
                    let mut t = node(TreeKind::Ident {
                        name: crate::materialize::RESOLVED_TYPE.to_string(),
                    });
                    t.ty = ty.clone();
                    return Ok(t);
                }
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
            // `Function(vparams, body)` and the `ValDef`s under it. slick's
            // `TableQueryMacroImpl.apply` builds exactly this -- a function
            // literal whose one parameter is spelled out with a `Modifiers`,
            // a `TermName` and a type `Ident` -- and hands it to
            // `TableQuery.apply[E]`.
            "Function" => {
                let mut vparams = Vec::new();
                for a in at(kids, 0)?.list()?.iter().skip(1) {
                    vparams.push(self.tree_from_reply(a, span)?);
                }
                let body = self.tree_from_reply(at(kids, 1)?, span)?;
                Ok(node(TreeKind::Function {
                    vparams,
                    body: Box::new(body),
                }))
            }
            "ValDef" => {
                let mods = mods_from(at(kids, 0)?)?;
                let name = decode_method_name(&name_from(at(kids, 1)?)?);
                // `q"val ff = $f"`: nsc's quasiquote writes an *empty*
                // `TypeTree` where the source wrote no type, and slick's
                // `mapToImpl` opens with two of them. Our parser leaves the
                // same hole for an inferred type, so it becomes `Empty` here
                // and the typer works the type out from the right-hand side.
                // Only in this position: an empty `TypeTree` anywhere else has
                // nothing in our AST that stands for "work it out", and is
                // still refused rather than turned into one.
                let tpt = if is_empty_type_tree(at(kids, 2)?) {
                    node(TreeKind::Empty)
                } else {
                    self.tree_from_reply(at(kids, 2)?, span)?
                };
                let rhs = self.tree_from_reply(at(kids, 3)?, span)?;
                Ok(node(TreeKind::ValDef {
                    mods,
                    name,
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                }))
            }
            "This" => Ok(node(TreeKind::This { qual: None })),
            "EmptyTree" => Ok(node(TreeKind::Empty)),
            other => Err(format!(
                "the expansion contains a `{other}`, which scala-rs cannot rebuild yet"
            )),
        }
    }
}

/// Whether the engine sent back a `TypeTree` with no type in it -- nsc's
/// spelling for "this type was not written; infer it".
fn is_empty_type_tree(s: &Sexp) -> bool {
    let Ok(items) = s.list() else {
        return false;
    };
    if items.first().and_then(|s| s.atom()) != Some("t")
        || at(items, 1).map(|s| s.text()).as_deref() != Ok("TypeTree")
    {
        return false;
    }
    let Ok(kids) = at(items, 3).and_then(|k| k.list()) else {
        return false;
    };
    kids.len() == 2 && at(kids, 1).map(|s| s.text()).as_deref() == Ok("")
}

/// The `Modifiers` of a `ValDef` the engine sent back.
///
/// Every flag arrives by *name*, so scala-rs never has to know nsc's bit
/// layout -- just as well, because several bits carry two names and a number
/// on the wire would make this guess. The ambiguous pairs are resolved for the
/// only definition this expander rebuilds, a `ValDef`: on a value the
/// `BYNAMEPARAM`/`COVARIANT` bit is by-name and the `DEFAULTPARAM`/`TRAIT` bit
/// is a default argument, so the type-parameter reading of each is dropped.
///
/// A name that is not in the table, and a leftover bit with no name at all,
/// are both diagnostics. A modifier dropped in silence would rebuild a
/// *different* definition -- a `var` as a `val`, a `lazy val` as a strict one
/// -- and nothing downstream would notice.
fn mods_from(s: &Sexp) -> Result<Modifiers, String> {
    let items = s.list()?;
    if items.first().and_then(|s| s.atom()) != Some("mods") {
        return Err(format!("expected modifiers, got {s:?}"));
    }
    let mut flags = Flags::EMPTY;
    for f in at(items, 1)?.list()?.iter().skip(1) {
        let name = f.text();
        let one = match name.as_str() {
            "PARAM" => Flags::PARAM,
            "IMPLICIT" => Flags::IMPLICIT,
            "LAZY" => Flags::LAZY,
            "MUTABLE" => Flags::MUTABLE,
            "FINAL" => Flags::FINAL,
            "PRIVATE" => Flags::PRIVATE,
            "PROTECTED" => Flags::PROTECTED,
            "LOCAL" => Flags::LOCAL,
            "OVERRIDE" => Flags::OVERRIDE,
            "BYNAMEPARAM" => Flags::BYNAME,
            "DEFAULTPARAM" => Flags::DEFAULTPARAM,
            "PRESUPER" => Flags::PRESUPER,
            "SYNTHETIC" => Flags::SYNTHETIC,
            // The second name on a bit already read above, and the two that
            // only record how nsc produced the definition.
            "COVARIANT" | "TRAIT" | "ARTIFACT" | "STABLE" => Flags::EMPTY,
            other => {
                return Err(format!(
                    "the expansion contains a definition marked `{other}`, \
                     a modifier scala-rs cannot rebuild yet"
                ))
            }
        };
        flags = flags.with(one);
    }
    let rest = at(items, 2)?.list()?;
    let rest = at(rest, 1)?.text();
    if rest != "0" {
        return Err(format!(
            "the expansion contains a definition with unnamed modifier bits \
             (0x{rest}), which scala-rs cannot rebuild"
        ));
    }
    let within = at(items, 3)?.text();
    let annots = at(items, 4)?.list()?;
    if annots.len() > 1 {
        return Err("the expansion contains an annotated definition, \
                    which scala-rs cannot rebuild yet"
            .to_string());
    }
    Ok(Modifiers {
        flags,
        private_within: (!within.is_empty()).then_some(within),
        annotations: Vec::new(),
    })
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
        scala_ref: false,
        stable_pat: false,
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
            scala_ref: false,
            stable_pat: false,
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

/// How many `Apply` clauses `tree` carries.
fn apply_layers(tree: &Tree) -> usize {
    let mut n = 0;
    let mut t = tree;
    while let TreeKind::Apply { fun, .. } = &t.kind {
        n += 1;
        t = fun;
    }
    n
}

/// The symbol at the head of an application spine, the way
/// [`Typer::macro_symbol_of`] reads it.
fn head_symbol(t: &Tree) -> SymbolId {
    let mut h = t;
    while let TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } = &h.kind {
        h = fun;
    }
    if h.sym.is_none() {
        t.sym
    } else {
        h.sym
    }
}

/// The node inside `tree` that really is the application of macro `sym`: the
/// one whose head is `sym` and which carries exactly the macro def's own
/// `want` argument clauses.
///
/// Needed because a macro whose *result* is applied puts more layers on the
/// tree than the macro def has clauses, and the extra ones are not always
/// plain `Apply`s: applying a function value goes through an `apply`
/// selection the typer inserts, so `M.f(1, 2)` on a nullary `f` arrives as
/// `Apply(Select(Select(M, f), apply), args)`.
fn macro_application_node(tree: &mut Tree, sym: SymbolId, want: usize) -> Option<&mut Tree> {
    if apply_layers(tree) == want && head_symbol(tree) == sym {
        return Some(tree);
    }
    match &mut tree.kind {
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => {
            macro_application_node(fun, sym, want)
        }
        TreeKind::Select { qual, .. } => macro_application_node(qual, sym, want),
        _ => None,
    }
}

/// The argument clauses and explicit type arguments of a macro application,
/// outermost application last -- i.e. in source order.
fn peel_application(tree: &Tree) -> (Vec<Vec<Tree>>, Vec<Type>, Option<Tree>) {
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
    // `c.prefix` is the receiver of the macro application. `M.f(1)` has one;
    // an unqualified `f(1)` does not (nsc synthesises `This`, which is not a
    // tree this bridge can hand over), and that is said by name rather than
    // guessed at.
    let prefix = match &t.kind {
        TreeKind::Select { qual, .. } => Some((**qual).clone()),
        _ => None,
    };
    (argss, targs, prefix)
}

// ------------------------------------------------------------ our tree → wire

/// The template a bare name belongs to, when nsc would have typed the name as
/// `C.this.name`.
///
/// nsc's typer replaces an `Ident` that resolves to a member of an enclosing
/// class or object with a `Select` on `This`; only a *local* -- something a
/// method or a block owns -- stays an `Ident`. A member of a package object
/// or of a package is not qualified with `this` either, so a package owner
/// says no.
fn this_qualifier_of(st: &SymbolTable, sym: SymbolId) -> Option<String> {
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
        // A module class is `Test$` here and in the JVM, but nsc's *symbol*
        // for it is named `Test` and that is what `Test.this` prints as.
        SymKind::ModuleClass => Some(name.strip_suffix('$').unwrap_or(name).to_string()),
        _ => None,
    }
}

/// Write a tree the implementation reads as one the typer has already been
/// over: `c.prefix` and `c.macroApplication`.
///
/// The difference from [`tree_to_wire`] is the `this` qualifier. nsc hands a
/// macro *typed* trees, so `macros.foo` -- where `macros` is a `val` in
/// `object Test` -- arrives as `Test.this.macros.foo`, and five corpus tests
/// (`macro-term-declared-in-{anonymous,class-object,object-object,refinement}`
/// and `macro-expand-override`) print the prefix and expect the qualifier.
///
/// Argument trees are deliberately *not* written this way. They are sent so
/// that the implementation can splice them into its expansion, which is then
/// type-checked again at the call site -- where an unqualified name still
/// means what the source meant, and a `This` we did not resolve would not.
fn typed_tree_to_wire(st: &SymbolTable, t: &Tree, out: &mut String) -> Result<(), String> {
    match &t.kind {
        TreeKind::Ident { name } => match this_qualifier_of(st, t.sym) {
            Some(owner) => {
                out.push_str("(t \"Select\" (s0) (t \"This\" (s0) (n type ");
                quote_into(out, &owner);
                out.push_str(")) (n term ");
                quote_into(out, &encode_method_name(name));
                out.push_str("))");
                Ok(())
            }
            None => tree_to_wire(t, out),
        },
        TreeKind::Select { qual, name } => {
            out.push_str("(t \"Select\" (s0) ");
            typed_tree_to_wire(st, qual, out)?;
            out.push_str(" (n term ");
            quote_into(out, &encode_method_name(name));
            out.push_str("))");
            Ok(())
        }
        TreeKind::Apply { fun, args } => {
            out.push_str("(t \"Apply\" (s0) ");
            typed_tree_to_wire(st, fun, out)?;
            out.push_str(" (l");
            for a in args {
                out.push(' ');
                tree_to_wire(a, out)?;
            }
            out.push_str("))");
            Ok(())
        }
        _ => tree_to_wire(t, out),
    }
}

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
        TreeKind::This { qual } => {
            out.push_str("(t \"This\" (s0) (n type ");
            quote_into(out, qual.as_deref().unwrap_or(""));
            out.push_str("))");
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

/// Why an implementation's own verdict may not be repeated to the user.
///
/// A class this run is compiling goes over as a placeholder that carries its
/// name and nothing else ([`Typer::tag_descriptor`]). An implementation that
/// asks such a symbol what it *is* -- slick's `mapToImpl` opens with
/// `if (!rSym.isClass || !rSym.asClass.isCaseClass) c.abort(...)` -- is
/// answering about a symbol it was never shown, so neither its `abort` nor an
/// exception out of it says anything about the program being compiled.
/// Reporting the implementation's message verbatim would be a wrong error;
/// this says what actually happened instead.
fn placeholder_verdict(placeholders: &[String], msg: &str) -> Option<String> {
    if placeholders.is_empty() {
        return None;
    }
    let names = placeholders
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "the type argument {names} is a class this run is compiling, so the \
         implementation was handed a placeholder symbol carrying only its \
         name; it looked the class up and answered \"{msg}\", which says \
         nothing about this program. nsc has no such limit -- it expands in \
         its own universe, where the class being compiled is a real symbol"
    ))
}

/// The class symbol of a monomorphic class type, if that is what `ty` is.
///
/// Only such a type can travel as a placeholder: the placeholder carries a
/// name, and a name is all a class *is* to the engine.
fn plain_class_of(st: &crate::symbol::SymbolTable, ty: &Type) -> Option<SymbolId> {
    match ty {
        Type::Class { sym, args } if args.is_empty() => {
            matches!(st.get(*sym).kind, crate::symbol::SymKind::Class).then_some(*sym)
        }
        _ => None,
    }
}

/// A class's full Scala name, from the class file name scala-rs would give it.
///
/// `a/b/Outer$Inner` is `a.b.Outer.Inner`: the JVM separates an owner from a
/// nested class with `$` and a package from its contents with `/`, and Scala
/// spells both with a dot.
fn scala_full_name(st: &crate::symbol::SymbolTable, sym: SymbolId) -> String {
    st.jvm_internal(sym).replace(['/', '$'], ".")
}

/// The class name a type tag is rebuilt from on the engine's side.
fn static_tag_class(st: &crate::symbol::SymbolTable, ty: &Type) -> Result<String, String> {
    // `f(42)` types its argument as the *constant* type `42`; the tag nsc
    // builds for it is `Int`.
    let widened = match ty {
        Type::Constant(lit) => Type::lit_underlying(lit),
        other => other.clone(),
    };
    crate::materialize::static_class_name(st, &widened)
        .map_err(|why| format!("scala-rs cannot build a type tag for {why}"))
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
