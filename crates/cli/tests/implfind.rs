//! Minimal reproductions of the slick errors reported as "implicit not found" and
//! "member not accessible". Every one is a shape real scalac 2.13.16 accepts, and
//! all 8 cases are collected in `tests/fixtures/implfind.scala`.
//!
//! The roots (in most cases the wording of the diagnostic and the root differ):
//!
//! 1. **An applied abstract type member does not conform to its own upper bound.**
//!    Applying `type CT[T] <: TT[T]` to `CT[U]` compared against the upper bound
//!    still as `TT[T]` (`T` being `CT`'s own parameter). Since `CT[U] <: TT[U]` is
//!    false, the evidence the context bound introduced did not satisfy that bound --
//!    so the root is **subtyping**, not implicit search. slick's
//!    `implicitly[BaseColumnType[U]]` and `TypedType[Boolean]` are this.
//!    The `Applied` vs. everything-else rule in `crates/typer/src/symbol.rs`.
//!
//! 2. **A context bound's evidence type is not expanded through the self type.**
//!    `[U : BCT]` writes the bound as a bare name, so it never took `tree_to_type`'s
//!    "applied type" path and `expand_type_members` never ran. Inside the cake that
//!    disagrees with the body's `implicitly[BCT[U]]` (which the self type does make
//!    concrete), and the sole candidate stops matching the request.
//!    `Checker::expand_bound_evidence`.
//!
//! 3. **A companion object's `protected` member.** nsc's
//!    `accessWithin(ab) || accessWithinLinked(ab)` (`ab = sym.owner`) admits a
//!    protected member from inside the owner **or inside its companion**. We only
//!    looked at the subclass rule, so slick's
//!    `object ResultConverterCompiler { protected lazy val logger }` could not be
//!    read from the trait of the same name.
//!
//! 4. **A nested `private[pkg] object` / `class`.** `namer_enter_tmpl` did not record
//!    the `private_within` of a `ClassDef` / `ModuleDef`, so a qualified private was
//!    treated as a plain private. slick's `GetResult.GetUpdateValue` (a
//!    `private[jdbc] object`). The brief's guess that this was "private in a
//!    companion" was wrong; it is a dropped qualified private.
//!
//! 5. **An anonymous class's self alias.** The parser was throwing away the `base` of
//!    `new T { base => … }` (`parse_new` hardcoded `self_name: None`).
//!    slick `TableQuery`'s `not found: value base`.
//!
//! 6. **The function position of a constructor pattern.** In `typingConstructorPattern`
//!    nsc drops non-stable methods from the name-resolution candidates. slick's `Node`
//!    has a `final def :@ (newType: Type)`, and the extractor `object :@` is imported
//!    from `TypeUtil._`. The method shadowed the extractor, giving
//!    `not found: extractor :@`. `SymbolTable::lookup_extractor`.
//!
//! 7. **A Java `Object` return type.** nsc's `objToAny` widens only the *parameters*
//!    of a Java method to `Any`; return types, fields and type arguments stay `AnyRef`.
//!    We widened all of them, so `cv.unwrapped eq null` came out as
//!    `value eq is not a member of Any`.
//!
//! 8. **The members of `scala.collection.Map`.** The linking traits `prelude_hier`
//!    builds carry no members, and `get`/`contains`/`getOrElse`/`apply` existed only
//!    on `immutable.Map` / `mutable.Map`. slick's `ExpandTables`, which takes the
//!    abstract type, came out as `value contains is not a member of Map`.
//!    `crates/typer/src/prelude_implfind.rs`.
//!
//! As a by-product of these, a **pickled nested class** such as `Ref.Make[F]` used to
//! resolve to the companion object in type position and give
//! "`Make` does not take type parameters"; that is fixed too
//! (`Checker::lookup_qualified_type`).
//!
//! `implfind.scala` is scala-library mode only (the private runtime has no `Map`).
//! `implfind_bad.scala` is the far side of the access rules 3 and 4 relaxed: it checks
//! that shapes nsc rejects are still rejected.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-implfind-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java_verified(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails_with(name: &str, needles: &[&str], extra: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Typecheck all 8 cases together and produce the same stdout as nsc 2.13.16.
#[test]
fn fixtures_implfind_scala_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implfind: scala-library jar not obtainable");
        return;
    };
    let out = compile_fixture_with("implfind", &["--scala-library", jar.to_str().unwrap()]);
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        let got = run_java_verified(&cp);
        assert_eq!(got, expected_stdout("implfind"), "stdout mismatch");
    }
    let _ = fs::remove_dir_all(&out);
}

/// The two relaxed access rules are not relaxed too far. nsc rejects these two too.
#[test]
fn fixtures_implfind_bad_is_error() {
    compile_fails_with(
        "implfind_bad",
        &[
            "value hidden cannot be accessed as a member of Prot$ from Stranger",
            "value Inner cannot be accessed as a member of Outer$ from Outsider",
        ],
        &["--no-scala-library"],
    );
}
