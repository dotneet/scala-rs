//! Bare expression statements in a template body (SLS 5.1 / 5.3).
//!
//! `class A { println("ctorA") }` used to compile without a diagnostic and
//! then print nothing: the constructor emitters filtered the template body
//! down to its `ValDef`s, so every statement that was not a `val` / `var` /
//! `def` was silently dropped — out of the primary constructor, out of a
//! trait's `$init$` and out of a module's initializer alike. The `val`
//! initializers ran, the statements between them did not.
//!
//! A template-body statement is part of the template's *initializer*: for a
//! class it runs inside the primary constructor, for a trait inside `$init$`
//! (so at mixin time, in linearization order), for an `object` inside the
//! module constructor. In every case it is interleaved with the `val` / `var`
//! initializers in **declaration order**.
//!
//! Every runtime check here is a dual-run: the fixture's expected output is
//! what real scalac 2.13.16 prints, and the classfiles are verified with
//! `java -Xverify:all` against the real `scala-library` jar as well as against
//! the private runtime. The shape assertions were read off `javap -p -c` of
//! scalac's own output for the same sources.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-ctorstmt-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java").arg("-version").output().is_ok()
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile(name: &str, tag: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(tag);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} ({tag}) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn diagnostics(name: &str) -> String {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
    assert!(
        !output.status.success(),
        "{name} should not compile:\n{text}"
    );
    text
}

fn run_verified(out: &Path, cp_extra: Option<&Path>, what: &str) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all failed for {what}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Run the fixture in both ABIs and compare against the recorded scalac output.
fn check_both_abis(name: &str) {
    if !java_available() {
        return;
    }
    let exp = expected_stdout(name);

    let out = compile(name, &format!("{name}-priv"), &["--no-scala-library"]);
    assert_eq!(
        run_verified(&out, None, "private runtime"),
        exp,
        "private-runtime stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run for {name}: jar not present");
        return;
    };
    let out = compile(
        name,
        &format!("{name}-lib"),
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_verified(&out, Some(&jar), "scala-library ABI"),
        exp,
        "scala-library stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Compile the same fixture with the real scalac 2.13.16 and diff the two
/// programs' stdout. The recorded expectation has to agree with scalac too, so
/// a stale `expected/` file cannot hide a wrong initialization order.
fn scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff for {name}: scalac or jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac"));
    let status = Command::new(&sc)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (real-scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed for {name}: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference = String::from_utf8_lossy(&reference.stdout).to_string();
    assert_eq!(
        reference,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);

    let out = compile(
        name,
        &format!("{name}-vs-scalac"),
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_verified(&out, Some(&jar), "scala-library ABI"),
        reference,
        "stdout differs from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// --- runtime behaviour ----------------------------------------------------

/// The reported reproduction: a statement in a `class`, in a `trait`, in a
/// `trait` mixed into a class, and in an `object`; statements alternating with
/// `val`s in a class and in a trait; a trait whose body is nothing but a
/// statement; a `var` of the body assigned by a later statement of the same
/// body; and a module initializer running exactly once.
#[test]
fn fixtures_cs() {
    check_both_abis("cs");
}

/// The same fixture against the real scalac 2.13.16, which is where the
/// expected initialization order `A;T1;T2;B;` comes from.
#[test]
fn real_scalac_dual_run_cs() {
    scalac_dual_run("cs");
}

/// The statement shapes that actually turn up in code: an early `require` /
/// `assert` on the constructor arguments, `if` / `match` / `try` / `while` in
/// statement position, a lambda, a `case class` body, a local class, an
/// anonymous class, and a member `object` reached through `$outer`.
#[test]
fn fixtures_cs_forms() {
    check_both_abis("cs_forms");
}

#[test]
fn real_scalac_dual_run_cs_forms() {
    scalac_dual_run("cs_forms");
}

/// A template statement is type-checked like any other code — the bug was that
/// it was dropped after the typer, not that it was never looked at. Both the
/// class-body and the trait-body statement have to be reported.
#[test]
fn fixtures_cs_bad_is_error() {
    let text = diagnostics("cs_bad");
    assert!(
        text.contains("not found: value notAMethod"),
        "a statement in a class body must be type-checked:\n{text}"
    );
    assert!(
        text.contains("value noSuchMember is not a member of Int"),
        "a statement in a trait body must be type-checked:\n{text}"
    );
}

// --- emitted shape --------------------------------------------------------

/// scalac's shape: the statement is compiled into the primary constructor,
/// after the super call and the mixin `$init$`s. Reading `Main$B()` out of
/// `javap -c` of scalac's own output gives
/// `invokespecial Main$A.<init>` / `invokestatic Main$T1.$init$` /
/// `invokestatic Main$T2.$init$` / `invokevirtual Main$.note` — which is
/// exactly what we emit: `$init$` is a `static` method on the interface.
#[test]
fn a_class_statement_lands_in_the_primary_constructor() {
    let out = compile("cs", "shape-class", &["--no-scala-library"]);
    let a = fs::read(out.join("Main$A.class")).expect("Main$A.class");
    assert!(
        contains(&a, b"note"),
        "Main$A's constructor must call `note`; the body statement was dropped"
    );
    let b = fs::read(out.join("Main$B.class")).expect("Main$B.class");
    for needle in [&b"Main$T1"[..], &b"Main$T2"[..], &b"note"[..]] {
        assert!(
            contains(&b, needle),
            "Main$B's constructor must reference {:?}",
            String::from_utf8_lossy(needle)
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// A trait whose body holds nothing but a statement still needs a `$init$`
/// that runs it: `trait T1 { note("T1") }` once produced no initializer at
/// all and no implementing class ever called one. `$init$` is a `static`
/// method on the interface itself, as in nsc 2.13.
#[test]
fn a_statement_only_trait_still_gets_an_init() {
    let out = compile("cs", "shape-trait", &["--no-scala-library"]);
    let t1 = fs::read(out.join("Main$T1.class")).expect("Main$T1.class");
    assert!(
        contains(&t1, b"$init$") && contains(&t1, b"note"),
        "Main$T1 must hold a `$init$` that calls `note`"
    );
    assert!(
        !out.join("Main$T1$class.class").exists(),
        "no `T$class` holder may be emitted any more"
    );
    let _ = fs::remove_dir_all(&out);
}

/// An `object`'s body statement belongs to module initialization, so it is
/// emitted once, into the module's own constructor (scalac hoists both the
/// statement and the `val` store into `<clinit>` for a static module; either
/// way it runs exactly once, which `cs`'s `O.v + O.v` checks at runtime).
#[test]
fn a_module_statement_lands_in_the_module_initializer() {
    let out = compile("cs", "shape-module", &["--no-scala-library"]);
    let o = fs::read(out.join("Main$O$.class")).expect("Main$O$.class");
    assert!(
        contains(&o, b"note") && contains(&o, b"MODULE$"),
        "Main$O$'s initializer must call `note`"
    );
    let _ = fs::remove_dir_all(&out);
}

// --- the parser side of the same bug --------------------------------------

/// `val p: String` followed by a statement on the next line: the type parser
/// used to skip the line break unconditionally while looking for a `with` or a
/// refinement `{`, so the statement was glued onto the declaration as the
/// infix type `String println "x"` — a second way for a template statement to
/// disappear, and one that turned it into a bogus "not found: type +" too.
#[test]
fn a_declaration_does_not_swallow_the_next_statement() {
    let dir = tmp_dir("parse");
    let src = dir.join("Decl.scala");
    fs::write(
        &src,
        "trait A {\n  \
         val p: String\n  \
         println(\"x\" + p)\n\
         }\n\
         trait B {\n  \
         def q: String\n  \
         println(\"y\")\n\
         }\n",
    )
    .unwrap();
    let output = Command::new(bin())
        .args(["compile", src.to_str().unwrap(), "--parse"])
        .output()
        .expect("run scala-rs compile --parse");
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(output.status.success(), "parse failed:\n{text}");
    assert_eq!(
        text.matches("Ident println").count(),
        2,
        "each `println` must be its own statement, not part of the declared type:\n{text}"
    );
    assert!(
        !text.contains("AppliedType"),
        "no infix type may be built out of the statement that follows a declaration:\n{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}
