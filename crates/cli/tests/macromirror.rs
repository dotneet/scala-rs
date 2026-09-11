//! Reverse RPC: a macro implementation asking the *running compiler* a
//! question in the middle of its own expansion. `docs/macros.md` §7.20.
//!
//! Everything else the engine does it can do inside
//! `scala.reflect.runtime.universe`, looking things up on the macro classpath.
//! `c.typecheck` cannot: what a tree means depends on the scope the macro was
//! called from, and that scope only ever existed inside scala-rs. So the
//! expansion protocol became a conversation -- the engine writes `(q …)`,
//! scala-rs answers `(a …)` and goes back to waiting for the expansion -- and
//! the answer is computed by really typing the tree, in the real typer, at the
//! real call site.
//!
//! Two compilations, because nsc requires two: a macro implementation has to
//! come from an *earlier* run. `mtc_impl.scala` is compiled first,
//! `mtc_use.scala` second with the first one's output on the classpath.
//!
//! The check that matters is the **dual run**: real scalac 2.13.16 compiles
//! the same two files against each other and the two programs must print the
//! same thing. A `c.typecheck` that answered plausibly but wrongly would still
//! compile and still run; only the output would differ.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-macromirror-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    cached.is_file().then_some(cached)
}

fn prerequisites(tag: &str) -> bool {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() {
        eprintln!("skip {tag}: scala-library / scala-reflect not obtainable");
        return false;
    }
    true
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

/// Compile the named fixtures with scala-rs, against scala-reflect.jar plus
/// `extra`.
fn compile(names: &[&str], out: &Path, extra: &[&Path]) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library");
    let reflect = scala_reflect_jar().expect("scala-reflect");
    let mut cp = reflect.display().to_string();
    for e in extra {
        cp.push(':');
        cp.push_str(&e.display().to_string());
    }
    let mut command = Command::new(bin());
    command.arg("compile");
    for name in names {
        command.arg(fixtures_dir().join(format!("{name}.scala")));
    }
    command
        .args(["-d", out.to_str().unwrap(), "-cp", &cp, "--scala-library"])
        .arg(&jar)
        .output()
        .expect("run scala-rs compile")
}

fn run_main(cp: &str, what: &str) -> String {
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for {what}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The whole reverse channel, end to end: six macros that ask `c.typecheck`
/// something, expanded for real and executed, against real scalac's output.
///
/// Two of the seven lines are the mirror over the current run's own symbols:
/// `Marker` is a class *this compilation is defining*, so it has no class file
/// and the engine's `mirror.staticClass` could never find it. It reaches the
/// engine described -- parents, declarations and their types -- because
/// scala-rs was asked, at the moment the name had to be handed over.
#[test]
fn mtc_typecheck_expands_and_runs() {
    if !prerequisites("mtc_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("impl");
    let uses = tmp_dir("use");

    let out = compile(&["mtc_impl"], &impls, &[]);
    assert!(
        out.status.success(),
        "compile mtc_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile(&["mtc_use"], &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile mtc_use failed: {}",
        diagnostics(&out)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected/mtc_use.txt")).unwrap();
    assert_eq!(run_main(&cp, "mtc_use"), expected, "stdout mismatch");
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// Every question the mirror cannot answer is refused *by name*.
///
/// This is the half that keeps the other half honest. Four of these six call
/// sites are programs real scalac 2.13.16 compiles and runs (printing
/// `Int(1) / Int(1) / Int(1) / caught`); scala-rs answers none of them,
/// and each refusal says which capability was missing. An approximate answer
/// would compile here and be wrong, and nothing downstream would notice --
/// which is exactly what `docs/macros.md` §7.18 warns a half-built mirror
/// does.
#[test]
fn mtc_unanswerable_questions_are_named() {
    if !prerequisites("mtc_bad") {
        return;
    }
    let impls = tmp_dir("bad-impl");
    let uses = tmp_dir("bad-use");

    let out = compile(&["mtc_impl", "mtc_bad_impl"], &impls, &[]);
    assert!(
        out.status.success(),
        "compile mtc_bad_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile(&["mtc_bad"], &uses, &[&impls]);
    let text = diagnostics(&out);
    for want in [
        // `withImplicitViewsDisabled` / `withMacrosDisabled`: a typer mode
        // scala-rs has no switch for, named rather than ignored.
        "asked to disable implicit views, which scala-rs's typer has no switch for",
        "asked to disable macro expansion, which scala-rs's typer has no switch for",
        // An expected type asks a different question from the one that is sent.
        "was given the expected type `Any`",
        // PATTERNmode, named rather than read as TERMmode.
        "was asked for PATTERNmode, which scala-rs does not implement",
        // A `TypecheckException` cannot cross a `java.lang.reflect.Proxy`, so
        // an implementation that catches one did not see what nsc shows it.
        "scala-rs cannot hand a TypecheckException to an implementation",
        // A node the reply rebuilder has no scala-rs tree for, outside the
        // pattern it could stand in.
        "contains a `Star`, which scala-rs cannot rebuild yet",
        // A class with a `val` used to be the mirror's limit here (`class
        // Bag(val size: Int)`, "`size` is a field"). The mirror now answers in
        // nsc's shape, field and accessor both, so that call site left this
        // file; `crates/cli/tests/gbmac.rs` compares such answers with real
        // scalac's and pins the refusals that remain.
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    // Every call site is an error, and none of them is accepted.
    assert!(!out.status.success() || text.contains("error:"), "{text}");
    assert_eq!(
        text.matches("macro expansion is not implemented").count(),
        6,
        "every call site must be refused:\n{text}"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}
