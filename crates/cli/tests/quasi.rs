//! Quasiquotes (`q"..."` / `tq"..."` / `pq"..."` / `cq"..."`) and the
//! reflection-API groundwork they sit on. See `docs/macros.md` §6.2 and
//! `crates/typer/src/quasiquote.rs`.
//!
//! Quasiquotes are **not** ordinary library macros: `scala-reflect.jar` holds
//! no implementation for them, so nsc short-circuits to a compiler-internal
//! one and scala-rs has to reify them itself. A subset is reified
//! (`crates/typer/src/reify.rs`); what these tests pin down is:
//!
//! 1. every quasiquote is diagnosed at its own span, distinguishing "this body
//!    uses syntax scala-rs cannot parse" from "the body is fine, the
//!    reification is missing" -- and a *user-defined* `q` interpolator is left
//!    alone;
//! 2. the pieces of the reflection API reached on the way there work, verified
//!    by a dual run against the real scalac: package-object members, `import
//!    <a value>._`, and applying a parameterless `def` whose result has an
//!    `apply` (`def Literal: LiteralExtractor`, then `Literal(...)`);
//! 3. with scala-reflect.jar on the classpath, `import <a universe>._` reaches
//!    the names it offers, and a macro *implementation* -- `c.Tree`,
//!    `c.Expr[T]`, `c.WeakTypeTag[T]`, `import c.universe._`, quasiquotes in
//!    its body -- compiles, to a class file that loads and verifies. Without
//!    that jar the placeholder `Context` stays and says what is missing.

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
        "scala-rs-quasi-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

fn compile_lib(name: &str, out: &Path, jar: &Path) -> std::process::Output {
    Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

/// Compiling `name` must fail, with every needle in the diagnostics.
fn compile_fails_lib_all(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not obtainable");
        return;
    };
    let out = tmp_dir(name);
    let output = compile_lib(name, &out, &jar);
    let err = diagnostics(&output);
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail, got: {err}"
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// --- the groundwork, dual-run ---------------------------------------------

/// `quasi.scala` under our compiler: the recorded stdout has to match exactly.
///
/// It exercises the three gaps that stood between scala-rs and
/// `scala.reflect`'s universe: a package object's members read from a jar
/// (`scala.math.Pi` is a `val` on `scala/math/package$`, and the package it is
/// folded into has no runtime value -- this used to emit an `invokevirtual`
/// with nothing on the stack), `import <a value>._` (which is how `import
/// c.universe._` works at all), and applying a parameterless `def` whose
/// result has an `apply`.
#[test]
fn scala_library_dual_run_quasi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip quasi dual-run: scala-library jar not obtainable");
        return;
    };
    let out = tmp_dir("quasi");
    let output = compile_lib("quasi", &out, &jar);
    assert!(
        output.status.success(),
        "compile quasi failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for quasi: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("quasi"),
        "stdout mismatch for quasi"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded expectation
/// and both compilers' output have to agree, or the fixture is only testing
/// what we happen to do.
#[test]
fn real_scalac_dual_run_quasi() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip quasi real-scalac diff: scalac or jar not obtainable");
        return;
    };
    let ref_out = tmp_dir("quasi-scalac-ref");
    let status = Command::new(&scalac)
        .args([
            fixtures_dir().join("quasi.scala").to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
        ])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile quasi");
    let cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed for quasi: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout("quasi"),
        "recorded expectation for quasi does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

// --- the quasiquotes themselves, all diagnosed ----------------------------

/// Every quasiquote in `quasi_bad.scala` is reported, and reported for the
/// right reason. Silently accepting one would be worse than not having them:
/// the call site would type against a tree we never built.
#[test]
fn fixtures_quasi_bad_is_error() {
    compile_fails_lib_all(
        "quasi_bad",
        &[
            // A body with nothing to reify: the *syntax* is what is wrong.
            "unimplemented syntax: quasiquote q\"...\" (empty quasiquote)",
            // A body that parses: what is missing is reification.
            "cannot expand quasiquote q\"...\"",
            "cannot expand quasiquote tq\"...\"",
            "cannot expand quasiquote pq\"...\"",
            "cannot expand quasiquote cq\"...\"",
            "docs/macros.md",
        ],
    );
}

/// The old diagnostic was `value q is not a member of StringContext`, which is
/// simply untrue -- `q` is a member of `Quasiquotes.Quasiquote` -- and points
/// at the wrong thing to fix. It must not come back.
#[test]
fn quasiquote_is_not_reported_as_a_stringcontext_member() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let out = tmp_dir("quasi_bad_msg");
    let err = diagnostics(&compile_lib("quasi_bad", &out, &jar));
    for prefix in ["q", "tq", "pq", "cq"] {
        assert!(
            !err.contains(&format!("value {prefix} is not a member of StringContext")),
            "quasiquote {prefix} still reported as a StringContext member: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// --- the reflect universe and macro `Context`, read from scala-reflect.jar --
//
// `docs/macros.md` §7.6. Everything below needs scala-reflect.jar: the macro
// API lives there, and `--scala-library` alone does not put it on the
// classpath. Without it scala-rs installs the placeholder `Context` of
// `crates/typer/src/prelude_reflect.rs` and says so; the last test pins that.

fn scala_reflect_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    cached.is_file().then_some(cached)
}

/// Compile `<name>.scala` against scala-reflect.jar.
fn compile_reflect(name: &str, out: &Path, jar: &Path, reflect: &Path) -> std::process::Output {
    Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            reflect.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

/// `tests/fixtures/qq_universe.scala`, run.
///
/// `import <a reflect universe>._` used to bring in nothing at all: the
/// universe's names are declared far up its linearisation (`TermName` on
/// `scala.reflect.api.Names`, `Literal` on `Trees`, `termNames` on
/// `StandardNames`), and a jar class's members are read one at a time, so no
/// completion had ever run for them. Reified quasiquotes did not notice,
/// because they build `u.TermName(...)` through the path.
///
/// It also pins the scope of the prefix such an import is qualified with: the
/// method-local `import u._` must not qualify anything after that method,
/// which used to emit a `getfield` for a dead local.
#[test]
fn qq_universe_wildcard_import_reaches_inherited_members() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip qq_universe: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("qq_universe");
    let output = compile_reflect("qq_universe", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile qq_universe failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}:{}", out.display(), reflect.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for qq_universe: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("qq_universe"),
        "stdout mismatch for qq_universe"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16: the trees have to be the same
/// trees, or the recorded expectation only records what we happen to build.
#[test]
fn qq_universe_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect), Some(scalac)) =
        (scala_library_jar(), scala_reflect_jar(), find_scalac())
    else {
        eprintln!("skip qq_universe scalac diff: scalac / jars not obtainable");
        return;
    };
    let ref_out = tmp_dir("qq_universe-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("qq_universe.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected qq_universe.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}",
        ref_out.display(),
        reflect.display(),
        jar.display()
    );
    let reference = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout("qq_universe"),
        "recorded expectation for qq_universe does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// `tests/fixtures/qq_ctx.scala`: a macro *implementation* compiles.
///
/// `c.Tree` / `c.Expr[T]` / `c.WeakTypeTag[T]` are type aliases the macro
/// `Context` inherits from `scala.reflect.macros.Aliases`, and a jar class's
/// type members had no completion path at all -- so a macro implementation
/// could not even state its own signature, and `import c.universe._` put no
/// universe in scope for the quasiquotes in its body. Real scalac 2.13.16
/// compiles the same file, so the fixture is known-good Scala rather than
/// something we happen to accept, and the class file we emit has to load and
/// verify.
#[test]
fn qq_ctx_macro_implementation_compiles() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip qq_ctx: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("qq_ctx");
    let output = compile_reflect("qq_ctx", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile qq_ctx failed: {}",
        diagnostics(&output)
    );

    if java_available() {
        // Loading a class links and verifies it, which is the check available
        // here: expanding these needs the JVM bridge, which is not built
        // (`docs/macros.md` §2.2). The trees their bodies build are checked
        // against scalac's in `qq_universe.scala`.
        let loader = out.join("loader");
        fs::create_dir_all(&loader).unwrap();
        let src = out.join("Loader.scala");
        fs::write(
            &src,
            "object Main {\n  \
             def main(args: Array[String]): Unit = println(Class.forName(\"QqCtx$\").getName)\n\
             }\n",
        )
        .unwrap();
        let built = Command::new(bin())
            .args([
                "compile",
                src.to_str().unwrap(),
                "-d",
                loader.to_str().unwrap(),
                "--scala-library",
                jar.to_str().unwrap(),
            ])
            .output()
            .expect("run scala-rs compile");
        assert!(
            built.status.success(),
            "compiling the loader failed: {}",
            diagnostics(&built)
        );
        let cp = format!(
            "{}:{}:{}:{}",
            loader.display(),
            out.display(),
            reflect.display(),
            jar.display()
        );
        let run = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "Main"])
            .output()
            .expect("java");
        assert!(
            run.status.success(),
            "the macro implementation's class file does not verify: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "QqCtx$\n");
    }

    let Some(scalac) = find_scalac() else {
        eprintln!("skip the scalac half of qq_ctx: scalac 2.13 not obtainable");
        let _ = fs::remove_dir_all(&out);
        return;
    };
    let ref_out = tmp_dir("qq_ctx-scalac");
    let built = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("qq_ctx.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        built.status.success(),
        "real scalac rejected qq_ctx.scala: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&out);
}

/// Every form a macro implementation's quasiquote cannot be reified into is
/// reported, naming the form.
///
/// This is the failure that would matter: reifying `q"$x : Int"` as `$x`
/// would compile, and expand to a tree nobody wrote.
#[test]
fn qq_ctx_bad_names_every_form_it_cannot_build() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip qq_ctx_bad: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("qq_ctx_bad");
    let output = compile_reflect("qq_ctx_bad", &out, &jar, &reflect);
    assert!(!output.status.success(), "expected qq_ctx_bad to fail");
    let err = diagnostics(&output);
    for needle in [
        "a right-associative operator (`::`) is not reified yet",
        "an `if` without an `else` is not reified yet",
        "a `_` placeholder function literal is not reified yet",
        "a by-name type is not reified yet",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    // A hole that is not a `Tree` (nsc lifts it with an implicit `Liftable`)
    // is a type error, never a silently different tree.
    assert!(
        err.contains("SyntacticApplied"),
        "an unliftable hole must still be an error: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Without scala-reflect.jar the placeholder `Context` stays, and says so.
///
/// The macro API is not in scala-library.jar, so `--scala-library` on its own
/// cannot type a macro implementation. The honest answer is `value universe is
/// not a member of Context`, not a `Context` that quietly has everything.
#[test]
fn qq_ctx_without_scala_reflect_is_diagnosed() {
    compile_fails_lib_all(
        "qq_ctx",
        &[
            "value universe is not a member of Context",
            "type Tree is not a member of Context",
            "type Expr is not a member of Context",
            "type WeakTypeTag is not a member of Context",
        ],
    );
}

// --- the rest of the quasiquote forms (`agent/reify2`, docs/macros.md §7.7) --

/// `tests/fixtures/qr_forms.scala`, run: `tq"..."`, `pq"..."`, `cq"..."` and
/// the `q"..."` shapes reification did not build before -- ascriptions, eta
/// expansion, blocks, `new`, `match`, function literals -- plus the tree
/// factories that are overload sets (`Ident`, `Bind`, `This`, `New`).
///
/// Every line prints `showRaw`, so what is compared is the *tree*, not the
/// fact that something typechecked. The recorded expectation is checked
/// against real scalac by `qr_forms_matches_real_scalac`.
#[test]
fn qr_forms_reifies_the_remaining_shapes() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip qr_forms: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("qr_forms");
    let output = compile_reflect("qr_forms", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile qr_forms failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}:{}", out.display(), reflect.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for qr_forms: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("qr_forms"),
        "stdout mismatch for qr_forms"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16. Without this the recorded
/// expectation would only say what we happen to build; with it, every
/// `Syntactic*` call in `crates/typer/src/reify.rs` is pinned to the tree
/// nsc's own quasiquote macro produces.
#[test]
fn qr_forms_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect), Some(scalac)) =
        (scala_library_jar(), scala_reflect_jar(), find_scalac())
    else {
        eprintln!("skip qr_forms scalac diff: scalac / jars not obtainable");
        return;
    };
    let ref_out = tmp_dir("qr_forms-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("qr_forms.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected qr_forms.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}",
        ref_out.display(),
        reflect.display(),
        jar.display()
    );
    let reference = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed for qr_forms: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout("qr_forms"),
        "recorded expectation for qr_forms does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// The forms reification still refuses, each named.
///
/// These are the ones where the parser normalises away something nsc keeps: a
/// right-associative operator, an `if` with no `else`, a `_` placeholder
/// lambda, a by-name type. Building *anything* for them would build a tree
/// nobody wrote, which is worse than not compiling.
#[test]
fn qr_forms_bad_names_every_form_it_cannot_build() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip qr_forms_bad: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("qr_forms_bad");
    let output = compile_reflect("qr_forms_bad", &out, &jar, &reflect);
    assert!(!output.status.success(), "expected qr_forms_bad to fail");
    let err = diagnostics(&output);
    for needle in [
        "a right-associative operator (`::`) is not reified yet",
        "an `if` without an `else` is not reified yet",
        "a `_` placeholder function literal is not reified yet",
        "a by-name type is not reified yet",
        "a `..$` splice mixed with ordinary arguments is not reified yet",
        "a class definition is not reified yet",
        "a modified `val` definition is not reified yet",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
