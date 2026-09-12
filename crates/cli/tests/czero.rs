//! cats at zero errors: the three things that stood between
//! `CATS_EXCLUDE='' tests/cats_measure.sh` and `errors=0`.
//!
//! 1. **`scala.collection.immutable.Iterable` was a supertype of nothing.**
//!    The immutable traits name it as their first parent in 2.13
//!    (`trait Set[A] extends Iterable[A] with collection.Set[A]`), and only the
//!    `collection.*` half of each edge was in `prelude_hier.rs`. With `import
//!    scala.collection.immutable._` in scope the bare name means that trait,
//!    which is how cats' `NonEmptySet.scala` writes `override def
//!    toIterable[A](fa: NonEmptySet[A]): Iterable[A] = fa.toSortedSet`.
//!    Fixture: `czero_immiter.scala`, 16 shapes, dual-run.
//!
//! 2. **A tag over an applied type constructor.**
//!    `(implicit evF: c.WeakTypeTag[F[Any]])` on `def lift[F[_], G[_]]` is a
//!    tag *for `F`* -- nsc reads `targ.typeSymbol`, which looks through the
//!    application -- and reading only the bare-parameter form made the whole
//!    trailing clause look like ordinary implicit values, so the macro
//!    definition was refused. Fixture: `czero_macrotag.scala`.
//!
//! 3. **Quasiquotes in pattern position.**
//!    `case q"($param) => $trans[..$typeArgs]($arg)"` is how
//!    `FunctionKMacros.scala` recognises the function it lifts.
//!    `crates/typer/src/quasi_pattern.rs` deconstructs the body the way nsc's
//!    `UnapplyReifier` does; `czero_quasipat.scala` runs 22 trees through 11
//!    such patterns against the runtime universe and must agree with real
//!    scalac line for line, and `czero_quasipat_bad.scala` pins that every body
//!    shape not covered is *reported*, never quietly matched.
//!
//! `czero_fk.scala` is cats' macro file itself, self-contained: all three at
//! once, plus the `forSome` existential and the `for (typeArg @ TypeTree() <-
//! typeArgs)` loop.
//!
//! Fixture prefix: `czero_`.

use std::fs;
use std::path::{Path, PathBuf};
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
        "scala-rs-czero-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

/// Compile `<name>.scala` with scala-rs (`scalac_path` `None`) or with the real
/// scalac, optionally with scala-reflect.jar on the classpath.
fn compile(
    name: &str,
    out: &Path,
    jar: &Path,
    reflect: Option<&Path>,
    scalac_path: Option<&Path>,
) -> std::process::Output {
    let src = fixtures_dir().join(format!("{name}.scala"));
    match scalac_path {
        Some(sc) => {
            let mut c = Command::new(sc);
            let cp = match reflect {
                Some(r) => format!("{}:{}", jar.display(), r.display()),
                None => jar.display().to_string(),
            };
            c.args(["-classpath", &cp, "-d", out.to_str().unwrap()])
                .arg(&src);
            c.output().expect("run scalac")
        }
        None => {
            let mut c = Command::new(bin());
            c.arg("compile")
                .arg(&src)
                .args(["-d", out.to_str().unwrap()])
                .args(["--scala-library", jar.to_str().unwrap()]);
            if let Some(r) = reflect {
                c.args(["-cp", r.to_str().unwrap()]);
            }
            c.output().expect("run scala-rs compile")
        }
    }
}

fn run_main(out: &Path, jar: &Path, reflect: Option<&Path>) -> String {
    let cp = match reflect {
        Some(r) => format!("{}:{}:{}", out.display(), r.display(), jar.display()),
        None => format!("{}:{}", out.display(), jar.display()),
    };
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java -Xverify:all Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name`, run it, and compare with the recorded expectation.
fn check_runs(name: &str, want_reflect: bool, scalac_path: Option<&Path>) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let reflect = if want_reflect {
        match scala_reflect_jar() {
            Some(r) => Some(r),
            None => {
                eprintln!("skip {name}: scala-reflect jar not present");
                return;
            }
        }
    } else {
        None
    };
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &out, &jar, reflect.as_deref(), scalac_path);
    assert!(
        output.status.success(),
        "compile of {name} failed:\n{}",
        diagnostics(&output)
    );
    assert_eq!(
        run_main(&out, &jar, reflect.as_deref()),
        expected_stdout(name),
        "stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Compile `name` and require success, without running it.
fn check_compiles(name: &str, scalac_path: Option<&Path>) {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip {name}: scala-library / scala-reflect not present");
        return;
    };
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &out, &jar, Some(&reflect), scalac_path);
    assert!(
        output.status.success(),
        "compile of {name} failed:\n{}",
        diagnostics(&output)
    );
    let _ = fs::remove_dir_all(&dir);
}

// --- 1. `scala.collection.immutable.Iterable` ------------------------------

#[test]
fn immutable_iterable_is_a_supertype_of_the_immutable_library() {
    check_runs("czero_immiter", false, None);
}

#[test]
fn scalac_agrees_immutable_iterable() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("czero_immiter", false, Some(&sc));
}

// --- 2. a tag over an applied type constructor -----------------------------

#[test]
fn macro_definition_accepts_a_tag_over_an_applied_constructor() {
    check_compiles("czero_macrotag", None);
}

#[test]
fn scalac_agrees_macro_tag_over_applied_constructor() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_compiles("czero_macrotag", Some(&sc));
}

// --- 3. quasiquote patterns ------------------------------------------------

#[test]
fn quasiquote_patterns_match_what_nsc_matches() {
    check_runs("czero_quasipat", true, None);
}

/// The same fixture through real scalac 2.13.16. Without this the recorded
/// expectation would only record what we happen to match.
#[test]
fn scalac_agrees_quasiquote_patterns() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("czero_quasipat", true, Some(&sc));
}

/// Every body shape the deconstructor does not cover is reported, naming the
/// shape. Accepting one silently would make a macro implementation compile and
/// then match the wrong trees -- or none -- at expansion time.
#[test]
fn unsupported_quasiquote_pattern_shapes_are_all_diagnosed() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip czero_quasipat_bad: scala-library / scala-reflect not present");
        return;
    };
    let dir = tmp_dir("czero_quasipat_bad");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile("czero_quasipat_bad", &out, &jar, Some(&reflect), None);
    let err = diagnostics(&output);
    assert!(
        !output.status.success(),
        "expected czero_quasipat_bad to fail, got: {err}"
    );
    for needle in [
        "a `..$` hole mixed with other elements in an argument list",
        "a parameter written out rather than spliced is not taken apart yet",
        "a right-associative operator (`::`) written infix",
        "`new C(\u{2026})` is not taken apart in pattern position yet",
        "an `if` is not taken apart in pattern position yet",
        "empty quasiquote",
        "a hole of rank 2 where rank 0 was expected",
        "tq\"...\" is not taken apart in pattern position yet",
        "a hole with a type ascription, which needs an `Unliftable` instance",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{err}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

// --- all three at once: cats' own macro file -------------------------------

#[test]
fn cats_function_k_macro_file_compiles() {
    check_compiles("czero_fk", None);
}

#[test]
fn scalac_agrees_cats_function_k_macro_file() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_compiles("czero_fk", Some(&sc));
}
