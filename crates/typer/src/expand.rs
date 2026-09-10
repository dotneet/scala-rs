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

use scala_rs_parser::{Flags, Lit, Modifiers, NodeId, SymbolId, Template, Tree, TreeKind, Type};
use scala_rs_pickle::names::{decode_method_name, encode_method_name};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::{MacroBinding, MacroTarg, SymKind, SymbolTable};

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

/// How many questions one expansion may ask scala-rs before it is presumed to
/// be looping (`crates/typer/src/expand_rpc.rs`). Answering costs the
/// implementation no time budget, so without this a loop that asks and
/// discards would run until the compiler was killed.
const MAX_ENGINE_QUERIES: u32 = 1024;

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
    /// Write one line to the engine. `Err` is a reason, already phrased for a
    /// user.
    pub(crate) fn send(&mut self, line: &str) -> Result<(), String> {
        if self.poisoned {
            return Err("the macro engine was shut down after an expansion \
                        timed out; later expansions in this run cannot be \
                        trusted and are not attempted"
                .to_string());
        }
        writeln!(self.stdin, "{line}").map_err(|e| format!("the macro engine died ({e})"))?;
        self.stdin
            .flush()
            .map_err(|e| format!("the macro engine died ({e})"))
    }

    /// Read one line the engine wrote, waiting at most what is left of
    /// `budget`, and take off it what the wait cost.
    ///
    /// The read runs on a helper thread so a wedged implementation costs a
    /// diagnostic and a killed child, not a process that never returns. Once
    /// timed out the engine is poisoned: the pipe still holds whatever that
    /// expansion eventually writes, so every later request would read the
    /// wrong reply.
    ///
    /// **The budget is the implementation's own time, not the wall clock of
    /// the conversation.** Since `agent/macromirror` the engine may stop
    /// mid-expansion to ask scala-rs a question ([`Typer::converse`]), and the
    /// time scala-rs spends answering is scala-rs's, not the macro's -- an
    /// implementation that asks a hundred questions must not be killed for
    /// how long *we* took. Only the intervals the engine holds the ball are
    /// subtracted, so the total an implementation gets is the same 20 seconds
    /// whether it asks nothing or asks a hundred times.
    pub(crate) fn read_reply(&mut self, budget: &mut Option<Duration>) -> Result<Sexp, String> {
        if self.poisoned {
            return Err("the macro engine was shut down after an expansion \
                        timed out; later expansions in this run cannot be \
                        trusted and are not attempted"
                .to_string());
        }
        let Some(limit) = *budget else {
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
        let started = Instant::now();
        let outcome = rx.recv_timeout(limit);
        *budget = Some(limit.saturating_sub(started.elapsed()));
        match outcome {
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
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            format!("cannot run `javac` to build the macro engine: {e}")
        })?;
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
            .map_err(|e| {
                let _ = std::fs::remove_dir_all(&staging);
                format!("cannot run `javac` to build the macro engine: {e}")
            })?;
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
        // The pipe carries one conversation at a time. Reaching here while a
        // query is being answered means a macro application inside the tree
        // an implementation handed to `c.typecheck`; nsc expands it (its typer
        // and its macro runner are the same process), and this bridge cannot
        // without a second engine and a second conversation.
        if self.macro_engine_busy {
            return Err("this macro application is inside a tree a macro \
                        implementation asked `c.typecheck` about, and the \
                        engine is already running that implementation; \
                        scala-rs does not expand a macro from inside another \
                        expansion's query yet"
                .to_string());
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
        self.macro_rpc_span = tree.span;
        self.macro_undescribed.clear();
        let reply = self.converse(&request)?;
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
                if let Some(why) =
                    crate::expand_rpc::undescribed_verdict(&self.macro_undescribed, &msg)
                {
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
                    if let Some(why) =
                        crate::expand_rpc::undescribed_verdict(&self.macro_undescribed, &msg)
                    {
                        return Err(why);
                    }
                }
                Err(msg)
            }
            _ => Err(format!("the macro engine replied {reply:?}")),
        }
    }

    /// Send one request and read the reply, answering whatever the engine
    /// asks along the way.
    ///
    /// The protocol used to be one line out, one line back. It is now a
    /// *conversation*: an expansion may stop and write `(q …)`, a question
    /// only scala-rs can answer -- what does this tree typecheck to, in the
    /// scope the macro was called from -- and wait for `(a …)` on its own
    /// stdin before going on. That is the reverse RPC `docs/macros.md` §7.18
    /// asks for, and it is what makes `c.typecheck` possible at all: the
    /// answer has to come from the run's own symbols, which live here and
    /// cannot be snapshotted into the engine.
    ///
    /// The engine is taken out of `self` for the duration, because answering
    /// needs `&mut self` -- the answer is computed by really typechecking, in
    /// the real typer, at the real call site. `macro_engine_busy` says so, and
    /// is what stops an expansion nested inside an answer from starting a
    /// second engine.
    fn converse(&mut self, request: &str) -> Result<Sexp, String> {
        let Some(mut engine) = self.macro_engine.take() else {
            return Err("the macro engine is not running".to_string());
        };
        let outer_busy = self.macro_engine_busy;
        self.macro_engine_busy = true;
        let mut budget = expansion_timeout();
        let result = self.converse_with(&mut engine, request, &mut budget);
        self.macro_engine_busy = outer_busy;
        self.macro_engine = Some(engine);
        result
    }

    fn converse_with(
        &mut self,
        engine: &mut MacroEngine,
        request: &str,
        budget: &mut Option<Duration>,
    ) -> Result<Sexp, String> {
        engine.send(request)?;
        // A wedged implementation is caught by the budget; a *chattering* one
        // -- an implementation whose questions never end although each is
        // answered quickly -- is not, because answering costs it no budget.
        // This is the same guard `MAX_EXPANSION_DEPTH` is for one level up.
        let mut asked = 0u32;
        loop {
            let reply = engine.read_reply(budget)?;
            let items = reply.list()?;
            if items.first().and_then(|s| s.atom()) != Some("q") {
                return Ok(reply);
            }
            asked += 1;
            if asked > MAX_ENGINE_QUERIES {
                return Err(format!(
                    "the macro implementation asked scala-rs more than \
                     {MAX_ENGINE_QUERIES} questions in one expansion; it is \
                     looping"
                ));
            }
            let answer = self.answer_query(items);
            engine.send(&answer)?;
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
        let sym = self
            .macro_symbol_of(application)
            .ok_or("macro application lost its symbol")?;
        let paramss = match &self.st.get(sym).ty {
            Type::Method { paramss, .. } => paramss.clone(),
            _ => Vec::new(),
        };
        // Typing may auto-apply trailing empty clauses without an Apply node.
        // Nonempty clauses must still be supplied by the typed application.
        if argss.len() > paramss.len()
            || paramss[argss.len()..]
                .iter()
                .any(|params| !params.is_empty())
        {
            return Err("macro argument clauses do not match its declaration".to_string());
        }
        out.push_str(" (argss");
        let mut slot = 0;
        for (clause_index, params) in paramss.iter().enumerate() {
            let clause = argss.get(clause_index).map(Vec::as_slice).unwrap_or(&[]);
            out.push_str(" (args");
            let repeated = params
                .last()
                .is_some_and(|p| matches!(p, Type::Repeated(_)));
            let fixed = params.len() - usize::from(repeated);
            if clause.len() < fixed || (!repeated && clause.len() != fixed) {
                return Err("macro arguments do not match its declaration".to_string());
            }
            for (index, _) in params.iter().enumerate() {
                let as_expr = *binding
                    .expr_args
                    .get(slot)
                    .ok_or("macro implementation argument metadata is incomplete")?;
                slot += 1;
                if repeated && index == fixed {
                    out.push_str(" (repeat");
                    for arg in &clause[index..] {
                        argument_to_wire(arg, as_expr, &mut out)?;
                    }
                    out.push(')');
                } else {
                    argument_to_wire(&clause[index], as_expr, &mut out)?;
                }
            }
            out.push(')');
        }
        if slot != binding.expr_args.len() {
            return Err(
                "macro implementation argument metadata does not match its declaration".to_string(),
            );
        }
        out.push_str(") (tags");
        if binding.tag_params > 0 {
            for t in self.tag_types(binding, targs, prefix)? {
                out.push(' ');
                let desc = self.tag_descriptor(&t, &mut placeholders)?;
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
        // Preserve actual source content and UTF-16 point offsets for macros
        // that inspect the source line, rather than only placing diagnostics.
        out.push_str(" (position ");
        if let Some(source) = self.sources.get(self.file_index) {
            let mut method = application;
            while let TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } = &method.kind {
                method = fun;
            }
            let mut byte = method.span.lo.to_usize().min(source.len());
            if let TreeKind::Select { qual, name } = &method.kind {
                let start = qual.span.hi.to_usize().min(source.len());
                let end = method.span.hi.to_usize().min(source.len());
                if let Some(offset) = source.get(start..end).and_then(|text| text.find(name)) {
                    byte = start + offset;
                }
            }
            let point = source
                .get(..byte)
                .ok_or("macro position splits a UTF-8 character")?
                .encode_utf16()
                .count();
            quote_into(&mut out, source);
            out.push(' ');
            quote_into(
                &mut out,
                self.source_paths
                    .get(self.file_index)
                    .map(String::as_str)
                    .unwrap_or("<macro-source>"),
            );
            out.push(' ');
            out.push_str(&point.to_string());
        } else {
            out.push_str("none");
        }
        out.push(')');
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

    /// The type each `WeakTypeTag` the implementation asks for stands for, at
    /// **this** call site.
    ///
    /// nsc does not line the tags up with the call site's type arguments. It
    /// resolves `MacroImplBinding.targs` -- the type arguments written on the
    /// implementation *reference*, `Impl.method[R, U]` on the macro def's
    /// right-hand side -- and each one separately (`Macros.macroArgs`): a type
    /// parameter of the macro def is looked up among the call site's type
    /// arguments, and anything else, a type parameter of the macro def's
    /// *owner* above all, is `asSeenFrom` the prefix. slick's
    /// `mapTo[R] = macro ShapedValue.mapToImpl[R, U]` is exactly that -- `U` is
    /// `ShapedValue`'s own type parameter -- so the call site writes one type
    /// argument where the implementation asks for two tags, and no lining-up
    /// could ever have worked.
    ///
    /// [`MacroBinding::tag_targs`] is that list, already classified where the
    /// macro def was bound. When it is empty the reference could not be read
    /// and the older rule stands: one call-site type argument per tag, and a
    /// refusal that says so when the counts differ.
    fn tag_types(
        &mut self,
        binding: &MacroBinding,
        targs: &[Type],
        prefix: Option<&Tree>,
    ) -> Result<Vec<Type>, String> {
        if binding.tag_targs.is_empty() {
            if targs.is_empty() {
                return Err(format!(
                    "the implementation asks for {} type tag(s) and the call site \
                     writes no type arguments; scala-rs does not pass an inferred \
                     type argument to a macro yet",
                    binding.tag_params
                ));
            }
            if targs.len() != binding.tag_params {
                return Err(format!(
                    "the implementation asks for {} type tag(s) and the call site \
                     supplies {} type argument(s); nsc would resolve the type \
                     arguments written on the implementation reference itself, and \
                     scala-rs could not read them off this macro def's reference",
                    binding.tag_params,
                    targs.len()
                ));
            }
            return Ok(targs.to_vec());
        }
        let mut out = Vec::with_capacity(binding.tag_targs.len());
        for want in &binding.tag_targs {
            out.push(match want {
                MacroTarg::DefParam { index, name } => match targs.get(*index) {
                    Some(t) => t.clone(),
                    None => {
                        return Err(format!(
                            "the implementation reference asks for a tag for `{name}`, \
                             the macro's own type parameter at position {}, and the \
                             call site writes {} type argument(s); scala-rs does not \
                             pass an inferred type argument to a macro yet",
                            index + 1,
                            targs.len()
                        ))
                    }
                },
                MacroTarg::OwnerParam { owner, index, name } => {
                    self.owner_tag_type(*owner, *index, name, prefix)?
                }
                MacroTarg::Fixed(t) => t.clone(),
                MacroTarg::Unresolved(what) => {
                    return Err(format!(
                        "the implementation reference writes `{what}` as a type \
                         argument, and scala-rs will not resolve that: nsc reads the \
                         written type's *symbol* and then that symbol's own type, so \
                         an applied type constructor reaches the implementation with \
                         the class's own type parameters in it rather than anything \
                         from this call site, and reproducing that would hand the \
                         implementation a type with a free parameter in it"
                    ))
                }
            });
        }
        Ok(out)
    }

    /// One tag whose type argument is a type parameter of the macro def's
    /// **owner**: nsc's `targ.tpe.asSeenFrom(prefix.tpe, macroDef.owner)`.
    ///
    /// The prefix is the receiver the macro was called on, so this is a base
    /// type of its type -- `ShapedValue[T, U]` reached through whatever the
    /// receiver actually is. Refused by name, not approximated, when the
    /// receiver is not a class type or does not have the owner among its base
    /// classes with arguments: an approximation here is a *wrong tag*, and a
    /// macro that builds a tree from one is wrong silently.
    fn owner_tag_type(
        &mut self,
        owner: SymbolId,
        index: usize,
        name: &str,
        prefix: Option<&Tree>,
    ) -> Result<Type, String> {
        let owner_name = self.st.get(owner).name.clone();
        let Some(p) = prefix else {
            return Err(format!(
                "the implementation reference asks for a tag for `{name}`, a type \
                 parameter of `{owner_name}`, which nsc reads off the receiver the \
                 macro was called on; this call has no receiver, and scala-rs does \
                 not synthesise the enclosing `this` for a prefix yet"
            ));
        };
        let Type::Class { sym, args } = &p.ty else {
            return Err(format!(
                "the implementation reference asks for a tag for `{name}`, a type \
                 parameter of `{owner_name}`, which nsc reads off the receiver the \
                 macro was called on; this receiver's type is `{}`, which is not a \
                 class applied to type arguments, and scala-rs will not guess what \
                 `{name}` stands for",
                self.st.display_type(&p.ty)
            ));
        };
        let seen = self.st.base_type_args(*sym, args);
        match seen.get(&owner.0).and_then(|a| a.get(index)) {
            Some(t) => Ok(t.clone()),
            None => Err(format!(
                "the implementation reference asks for a tag for `{name}`, the type \
                 parameter of `{owner_name}` at position {}, and scala-rs cannot see \
                 `{owner_name}` applied to type arguments in the receiver's type \
                 `{}`",
                index + 1,
                self.st.display_type(&p.ty)
            )),
        }
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
        // `f(42)` types its argument as the *constant* type `42`; the tag nsc
        // builds for the `Expr` that wraps it is `Int`. Only the outermost
        // type is widened -- `Tag[1]` is not `Tag[Int]`, so a constant that is
        // itself a type argument stays one ([`Typer::tag_wire`]).
        let widened = match ty {
            Type::Constant(lit) => Type::lit_underlying(lit),
            other => other.clone(),
        };
        self.tag_wire(&widened, placeholders)
    }

    /// One type as a tag descriptor, type arguments and all.
    ///
    /// `(ty "scala.reflect.ClassTag" (ty "a.b.Row"))` is `mirror.staticClass`
    /// applied to the arguments, written the same way, which is what
    /// `ScalaRsMacroEngine.typeFor` turns into `universe.appliedType`.
    ///
    /// Before this a descriptor was a bare name, and a tag for an applied type
    /// constructor could not be *requested* at all. That is where every one of
    /// gitbucket's 31 `mapTo` call sites stopped: `ShapedValue.mapToImpl`
    /// takes a `c.Expr[ClassTag[R]]`, so the request could not be built and
    /// the implementation was never invoked (`docs/macros.md` §7.20, §7.21).
    ///
    /// A type argument that is a class **this run is compiling** still travels
    /// as the empty placeholder of §5.1, at whatever depth it occurs.
    fn tag_wire(&mut self, ty: &Type, placeholders: &mut Vec<String>) -> Result<String, String> {
        if let Some(sym) = plain_class_of(&self.st, ty) {
            if self.is_current_run_class(sym) {
                let full = scala_full_name(&self.st, sym);
                self.macro_local_tags.insert(full.clone(), ty.clone());
                placeholders.push(full.clone());
                let mut out = String::from("(syn ");
                quote_into(&mut out, &full);
                out.push(')');
                return Ok(out);
            }
        }
        // A constant type nested inside a type argument. nsc's tag for
        // `Tag[1]` carries `Int(1)`, and widening it here would hand the
        // implementation a different type from the one it asked about.
        if let Type::Constant(lit) = ty {
            let mut out = String::from("(cst ");
            lit_to_wire(lit, &mut out)?;
            out.push(')');
            return Ok(out);
        }
        if let Some((name, args)) = self.applied_tag_shape(ty)? {
            let mut written = Vec::new();
            for a in &args {
                written.push(self.tag_wire(a, placeholders)?);
            }
            let mut out = String::from("(ty ");
            quote_into(&mut out, &name);
            for w in written {
                out.push(' ');
                out.push_str(&w);
            }
            out.push(')');
            return Ok(out);
        }
        let name = static_tag_class(&self.st, ty)?;
        let mut out = String::from("(ty ");
        quote_into(&mut out, &name);
        out.push(')');
        Ok(out)
    }

    /// A type constructor applied to arguments, as the class the engine's
    /// mirror resolves plus the arguments to apply it to.
    ///
    /// `Ok(None)` means "not an application"; the caller then writes the type
    /// as a plain name. `Err` is a refusal that names what could not be built,
    /// the way every other descriptor refusal does.
    ///
    /// The three structural shapes are written out because scala-rs models
    /// them as their own `Type` variants rather than as class applications,
    /// and nsc's tag for each is exactly the class named here: `(A, B)` is
    /// `scala.Tuple2[A, B]`, `A => B` is `scala.Function1[A, B]` and
    /// `Array[A]` is `scala.Array[A]`.
    fn applied_tag_shape(&mut self, ty: &Type) -> Result<Option<(String, Vec<Type>)>, String> {
        let named = |n: &str, args: Vec<Type>| Ok(Some((n.to_string(), args)));
        match ty {
            Type::Class { sym, args } if !args.is_empty() => {
                // A class this run is compiling has no class file for
                // `mirror.staticClass` to find, and the placeholder that
                // stands in for one carries a name and nothing else -- so its
                // type parameters would have nothing to bind. Refused by name
                // rather than sent as a name the mirror fails to resolve.
                if self.is_current_run_class(*sym) {
                    return Err(format!(
                        "scala-rs cannot build a type tag for `{}`, a class this run \
                         is compiling applied to type arguments; the placeholder the \
                         engine is given carries a name and nothing else",
                        scala_full_name(&self.st, *sym)
                    ));
                }
                let name = crate::materialize::static_class_of_sym(&self.st, *sym)
                    .map_err(|why| format!("scala-rs cannot build a type tag for {why}"))?;
                Ok(Some((name, args.clone())))
            }
            Type::Tuple(ts) if (1..=22).contains(&ts.len()) => {
                named(&format!("scala.Tuple{}", ts.len()), ts.clone())
            }
            Type::Function { params, ret } if params.len() <= 22 => {
                let mut args = params.clone();
                args.push((**ret).clone());
                named(&format!("scala.Function{}", params.len()), args)
            }
            Type::Array(t) => named("scala.Array", vec![(**t).clone()]),
            _ => Ok(None),
        }
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

    fn reply_trees(&mut self, s: &Sexp, span: Span) -> Result<Vec<Tree>, String> {
        s.list()?
            .iter()
            .skip(1)
            .map(|t| self.tree_from_reply(t, span))
            .collect()
    }

    /// Rebuild the reflect tree the engine wrote as an *untyped* scala-rs
    /// tree, ready to be typechecked at the call site.
    pub(crate) fn tree_from_reply(&mut self, s: &Sexp, span: Span) -> Result<Tree, String> {
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
        let id = NodeId(self.macro_next_node);
        self.macro_next_node = self
            .macro_next_node
            .checked_add(1)
            .ok_or("macro node identity exhausted")?;
        let node = |kind| Tree {
            id,
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
            "TypeApply" | "AppliedTypeTree" => {
                let fun = self.tree_from_reply(at(kids, 0)?, span)?;
                let mut args = Vec::new();
                for a in at(kids, 1)?.list()?.iter().skip(1) {
                    args.push(self.tree_from_reply(a, span)?);
                }
                Ok(node(if kind == "AppliedTypeTree" {
                    TreeKind::AppliedTypeTree {
                        tpt: Box::new(fun),
                        args,
                    }
                } else {
                    TreeKind::TypeApply {
                        fun: Box::new(fun),
                        args,
                    }
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
            "Assign" => Ok(node(TreeKind::Assign {
                lhs: Box::new(self.tree_from_reply(at(kids, 0)?, span)?),
                rhs: Box::new(self.tree_from_reply(at(kids, 1)?, span)?),
            })),
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
            "Annotated" => {
                let tpt = if is_empty_type_tree(at(kids, 1)?) {
                    node(TreeKind::Empty)
                } else {
                    self.tree_from_reply(at(kids, 1)?, span)?
                };
                Ok(node(TreeKind::AnnotatedTypeTree {
                    annot: Box::new(self.tree_from_reply(at(kids, 0)?, span)?),
                    tpt: Box::new(tpt),
                }))
            }
            "ClassDef" => {
                let mods = mods_from(at(kids, 0)?)?;
                let name = name_from(at(kids, 1)?)?;
                let tparams = self.reply_trees(at(kids, 2)?, span)?;
                let template = at(kids, 3)?.list()?;
                if at(template, 1)?.text() != "Template" {
                    return Err("macro ClassDef has no Template".to_string());
                }
                let parents = self.reply_trees(at(template, 3)?, span)?;
                let self_def = at(template, 4)?.list()?;
                let self_name = name_from(at(self_def, 4)?)?;
                let self_tpt = at(self_def, 5)?;
                let self_tpt = if is_empty_type_tree(self_tpt) {
                    None
                } else {
                    Some(Box::new(self.tree_from_reply(self_tpt, span)?))
                };
                let mut body = Vec::new();
                let mut ctor_mods = Modifiers::default();
                let mut vparamss = Vec::new();
                let mut primary = false;
                for member in at(template, 5)?.list()?.iter().skip(1) {
                    let fields = member.list()?;
                    if !primary
                        && at(fields, 1)?.text() == "DefDef"
                        && name_from(at(fields, 4)?)? == "<init>"
                    {
                        primary = true;
                        ctor_mods = mods_from(at(fields, 3)?)?;
                        for clause in at(fields, 6)?.list()?.iter().skip(1) {
                            vparamss.push(self.reply_trees(clause, span)?);
                        }
                        // The primary constructor's super call is represented
                        // by Template.parents in our AST. Preserve additional
                        // constructor statements as template initialization.
                        let rhs = at(fields, 8)?.list()?;
                        if at(rhs, 1)?.text() != "Block" {
                            return Err("macro primary constructor is not a block".to_string());
                        }
                        let stats = at(rhs, 3)?.list()?;
                        for (i, stat) in stats.iter().skip(1).enumerate() {
                            if i == 0 && empty_super_call(stat) {
                                continue;
                            }
                            body.push(self.tree_from_reply(stat, span)?);
                        }
                        let result = self.tree_from_reply(at(rhs, 4)?, span)?;
                        if !matches!(
                            result.kind,
                            TreeKind::Literal { lit: Lit::Unit } | TreeKind::Empty
                        ) {
                            body.push(result);
                        }
                    } else {
                        body.push(self.tree_from_reply(member, span)?);
                    }
                }
                Ok(node(TreeKind::ClassDef {
                    mods,
                    name,
                    tparams,
                    ctor_mods,
                    vparamss,
                    impl_: Template {
                        parents,
                        self_name: (!self_name.is_empty() && self_name != "_").then_some(self_name),
                        self_tpt,
                        body,
                        span,
                    },
                }))
            }
            "DefDef" => {
                let mods = mods_from(at(kids, 0)?)?;
                let name = decode_method_name(&name_from(at(kids, 1)?)?);
                let tparams = self.reply_trees(at(kids, 2)?, span)?;
                let mut vparamss = Vec::new();
                for clause in at(kids, 3)?.list()?.iter().skip(1) {
                    vparamss.push(self.reply_trees(clause, span)?);
                }
                let tpt = if is_empty_type_tree(at(kids, 4)?) {
                    node(TreeKind::Empty)
                } else {
                    self.tree_from_reply(at(kids, 4)?, span)?
                };
                let rhs = self.tree_from_reply(at(kids, 5)?, span)?;
                Ok(node(TreeKind::DefDef {
                    mods,
                    name,
                    tparams,
                    vparamss,
                    tpt: Box::new(tpt),
                    rhs: Box::new(rhs),
                }))
            }
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
            "This" => {
                let qual = name_from(at(kids, 0)?)?;
                Ok(node(TreeKind::This {
                    qual: (!qual.is_empty()).then_some(qual),
                }))
            }
            "EmptyTree" => Ok(node(TreeKind::Empty)),
            other => Err(format!(
                "the expansion contains a `{other}`, which scala-rs cannot rebuild yet"
            )),
        }
    }
}

/// Whether the engine sent back a `TypeTree` with no type in it -- nsc's
/// spelling for "this type was not written; infer it".
fn empty_super_call(s: &Sexp) -> bool {
    (|| -> Result<bool, String> {
        let app = s.list()?;
        if at(app, 1)?.text() != "Apply" || at(app, 4)?.list()?.len() != 1 {
            return Ok(false);
        }
        let select = at(app, 3)?.list()?;
        if at(select, 1)?.text() != "Select" || name_from(at(select, 4)?)? != "<init>" {
            return Ok(false);
        }
        Ok(at(at(select, 3)?.list()?, 1)?.text() == "Super")
    })()
    .unwrap_or(false)
}

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
pub(crate) fn at(items: &[Sexp], i: usize) -> Result<&Sexp, String> {
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
            if matches!(fun.kind, TreeKind::New { .. }) {
                application_fun_to_wire(fun, out)?;
            } else {
                typed_tree_to_wire(st, fun, out)?;
            }
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
pub(crate) fn tree_to_wire(t: &Tree, out: &mut String) -> Result<(), String> {
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
            application_fun_to_wire(fun, out)?;
            out.push_str(" (l");
            for a in args {
                out.push(' ');
                tree_to_wire(a, out)?;
            }
            out.push_str("))");
            Ok(())
        }
        TreeKind::Empty => {
            out.push_str("(t \"EmptyTree\" (s0))");
            Ok(())
        }
        TreeKind::Block { stats, expr } => {
            out.push_str("(t \"Block\" (s0) ");
            trees_to_wire(stats, out)?;
            out.push(' ');
            tree_to_wire(expr, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Function { vparams, body } => {
            out.push_str("(t \"Function\" (s0) ");
            trees_to_wire(vparams, out)?;
            out.push(' ');
            tree_to_wire(body, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::New { tpt } => {
            out.push_str("(t \"New\" (s0) ");
            type_tree_to_wire(tpt, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::If { cond, thenp, elsep } => {
            out.push_str("(t \"If\" (s0) ");
            tree_to_wire(cond, out)?;
            out.push(' ');
            tree_to_wire(thenp, out)?;
            out.push(' ');
            tree_to_wire(elsep, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Assign { lhs, rhs } => {
            out.push_str("(t \"Assign\" (s0) ");
            tree_to_wire(lhs, out)?;
            out.push(' ');
            tree_to_wire(rhs, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Typed { expr, tpt } => {
            out.push_str("(t \"Typed\" (s0) ");
            tree_to_wire(expr, out)?;
            out.push(' ');
            type_tree_to_wire(tpt, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::TypeApply { fun, args } => {
            out.push_str("(t \"TypeApply\" (s0) ");
            tree_to_wire(fun, out)?;
            out.push_str(" (l");
            for arg in args {
                out.push(' ');
                type_tree_to_wire(arg, out)?;
            }
            out.push_str("))");
            Ok(())
        }
        TreeKind::AppliedTypeTree { .. } | TreeKind::AnnotatedTypeTree { .. } => {
            type_tree_to_wire(t, out)
        }
        TreeKind::ValDef {
            mods,
            name,
            tpt,
            rhs,
        } => {
            out.push_str("(t \"ValDef\" (s0) ");
            mods_to_wire(mods, out)?;
            out.push_str(" (n term ");
            quote_into(out, &encode_method_name(name));
            out.push_str(") ");
            type_tree_to_wire(tpt, out)?;
            out.push(' ');
            tree_to_wire(rhs, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::ClassDef {
            mods,
            name,
            tparams,
            ctor_mods,
            vparamss,
            impl_,
        } => {
            out.push_str("(t \"ClassDef\" (s0) ");
            mods_to_wire(mods, out)?;
            out.push_str(" (n type ");
            quote_into(out, name);
            out.push_str(") ");
            trees_to_wire(tparams, out)?;
            out.push_str(" (t \"Template\" (s0) (l");
            for parent in &impl_.parents {
                if matches!(parent.kind, TreeKind::Apply { .. }) {
                    return Err(
                        "macro class transport cannot preserve superclass arguments yet"
                            .to_string(),
                    );
                }
                out.push(' ');
                type_tree_to_wire(parent, out)?;
            }
            out.push_str(") (t \"ValDef\" (s0) (mods (f) (rest \"0\") \"\" (l)) (n term ");
            quote_into(out, impl_.self_name.as_deref().unwrap_or("_"));
            out.push_str(") ");
            if let Some(tpt) = &impl_.self_tpt {
                type_tree_to_wire(tpt, out)?;
            } else {
                out.push_str("(t \"TypeTree\" (s0) (ty \"\"))");
            }
            out.push_str(" (t \"EmptyTree\" (s0))) (l (t \"DefDef\" (s0) ");
            mods_to_wire(ctor_mods, out)?;
            out.push_str(" (n term \"<init>\") (l) (l");
            for params in vparamss {
                out.push(' ');
                trees_to_wire(params, out)?;
            }
            out.push_str(") (t \"TypeTree\" (s0) (ty \"\")) (t \"Block\" (s0) (l (t \"Apply\" (s0) (t \"Select\" (s0) (t \"Super\" (s0) (t \"This\" (s0) (n type \"\")) (n type \"\")) (n term \"<init>\")) (l))) (t \"Literal\" (s0) (c \"Unit\" \"()\"))))");
            for stat in &impl_.body {
                out.push(' ');
                tree_to_wire(stat, out)?;
            }
            out.push_str(")))");
            Ok(())
        }
        TreeKind::DefDef {
            mods,
            name,
            tparams,
            vparamss,
            tpt,
            rhs,
        } => {
            out.push_str("(t \"DefDef\" (s0) ");
            mods_to_wire(mods, out)?;
            out.push_str(" (n term ");
            quote_into(out, &encode_method_name(name));
            out.push_str(") ");
            trees_to_wire(tparams, out)?;
            out.push_str(" (l");
            for clause in vparamss {
                out.push(' ');
                trees_to_wire(clause, out)?;
            }
            out.push_str(") ");
            type_tree_to_wire(tpt, out)?;
            out.push(' ');
            tree_to_wire(rhs, out)?;
            out.push(')');
            Ok(())
        }
        TreeKind::Match { .. } => unsupported("a `match`"),
        _ => unsupported("an argument of this form"),
    }
}

pub(crate) fn application_fun_to_wire(fun: &Tree, out: &mut String) -> Result<(), String> {
    if matches!(fun.kind, TreeKind::New { .. }) {
        out.push_str("(t \"Select\" (s0) ");
        tree_to_wire(fun, out)?;
        out.push_str(" (n term \"<init>\"))");
        Ok(())
    } else {
        tree_to_wire(fun, out)
    }
}

fn argument_to_wire(t: &Tree, as_expr: bool, out: &mut String) -> Result<(), String> {
    out.push_str(if as_expr {
        " (arg expr "
    } else {
        " (arg tree "
    });
    tree_to_wire(t, out)?;
    // nsc uses Expr[Nothing] for value arguments, not the argument's type.
    out.push_str(if as_expr {
        " (ty \"scala.Nothing\"))"
    } else {
        " (ty \"\"))"
    });
    Ok(())
}

fn trees_to_wire(trees: &[Tree], out: &mut String) -> Result<(), String> {
    out.push_str("(l");
    for tree in trees {
        out.push(' ');
        tree_to_wire(tree, out)?;
    }
    out.push(')');
    Ok(())
}

fn type_tree_to_wire(t: &Tree, out: &mut String) -> Result<(), String> {
    match &t.kind {
        TreeKind::Empty => {
            out.push_str("(t \"TypeTree\" (s0) (ty \"\"))");
            Ok(())
        }
        TreeKind::Ident { name } => {
            out.push_str("(t \"Ident\" (s0) (n type ");
            quote_into(out, name);
            out.push_str("))");
            Ok(())
        }
        TreeKind::Select { qual, name } => {
            out.push_str("(t \"Select\" (s0) ");
            tree_to_wire(qual, out)?;
            out.push_str(" (n type ");
            quote_into(out, name);
            out.push_str("))");
            Ok(())
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            out.push_str("(t \"AppliedTypeTree\" (s0) ");
            type_tree_to_wire(tpt, out)?;
            out.push_str(" (l");
            for arg in args {
                out.push(' ');
                type_tree_to_wire(arg, out)?;
            }
            out.push_str("))");
            Ok(())
        }
        TreeKind::AnnotatedTypeTree { tpt, annot } => {
            out.push_str("(t \"Annotated\" (s0) ");
            tree_to_wire(annot, out)?;
            out.push(' ');
            type_tree_to_wire(tpt, out)?;
            out.push(')');
            Ok(())
        }
        _ => tree_to_wire(t, out),
    }
}

fn mods_to_wire(mods: &Modifiers, out: &mut String) -> Result<(), String> {
    out.push_str("(mods (f");
    let mut known = Flags::EMPTY;
    for (flag, name) in [
        (Flags::PRIVATE, "PRIVATE"),
        (Flags::PROTECTED, "PROTECTED"),
        (Flags::ABSTRACT, "ABSTRACT"),
        (Flags::FINAL, "FINAL"),
        (Flags::SEALED, "SEALED"),
        (Flags::IMPLICIT, "IMPLICIT"),
        (Flags::LAZY, "LAZY"),
        (Flags::OVERRIDE, "OVERRIDE"),
        (Flags::CASE, "CASE"),
        (Flags::TRAIT, "TRAIT"),
        (Flags::MUTABLE, "MUTABLE"),
        (Flags::PARAM, "PARAM"),
        (Flags::BYNAME, "BYNAMEPARAM"),
        (Flags::DEFAULTPARAM, "DEFAULTPARAM"),
        (Flags::SYNTHETIC, "SYNTHETIC"),
        (Flags::LOCAL, "LOCAL"),
    ] {
        if mods.flags.contains(flag) {
            out.push(' ');
            quote_into(out, name);
            known = known.with(flag);
        }
    }
    if mods.flags.0 & !known.0 != 0 {
        return Err(format!(
            "cannot send definition modifiers 0x{:x} to macro engine",
            mods.flags.0 & !known.0
        ));
    }
    out.push_str(") (rest \"0\") ");
    quote_into(out, mods.private_within.as_deref().unwrap_or(""));
    out.push(' ');
    trees_to_wire(&mods.annotations, out)?;
    out.push(')');
    Ok(())
}

pub(crate) fn lit_to_wire(lit: &Lit, out: &mut String) -> Result<(), String> {
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
pub(crate) fn scala_full_name(st: &crate::symbol::SymbolTable, sym: SymbolId) -> String {
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

    pub(crate) fn list(&self) -> Result<&Vec<Sexp>, String> {
        match self {
            Sexp::List(v) => Ok(v),
            other => Err(format!("the macro engine sent {other:?}")),
        }
    }

    pub(crate) fn atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(a) => Some(a),
            _ => None,
        }
    }

    /// The payload of an atom or string, whichever this is.
    pub(crate) fn text(&self) -> String {
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

pub(crate) fn quote_into(out: &mut String, s: &str) {
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
