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
        "an `if` without an `else` is not reified yet",
        "a by-name type is not reified yet",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    // A hole that is neither a `Tree` nor anything with a standard `Liftable`
    // is reported by its type, never turned into a silently different tree.
    // (`Int` used to stand here; `docs/macros.md` §7.8 lifts it now, so the
    // fixture asks for a `File`, which no standard instance covers.)
    assert!(
        err.contains("a hole of type `File` is not lifted"),
        "an unliftable hole must still be an error, naming its type: {err}"
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
        "an `if` without an `else` is not reified yet",
        "a by-name type is not reified yet",
        "a `..$` splice mixed with ordinary arguments is not reified yet",
        "a type definition is not reified yet",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -- definitions (`agent/defquasi`) ---------------------------------------

/// `tests/fixtures/dq_defs.scala`: `class` / `case class` / `trait` /
/// `object` / `def` / a modified `val`, reified.
///
/// The whole point is the `Modifiers`, which carry
/// `scala.reflect.internal.Flags` bits that are *not* the parser's numbering
/// (`PRIVATE` is bit 2 there and bit 0 in the parser), plus the parents nsc's
/// parser supplies for a source that does not write them -- `AnyRef`, and
/// `Product with Serializable` for every `case` class. Getting either wrong
/// builds a definition nobody wrote, which is why this compares `showRaw`.
#[test]
fn dq_defs_reifies_definitions() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip dq_defs: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("dq_defs");
    let output = compile_reflect("dq_defs", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile dq_defs failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}:{}", out.display(), reflect.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for dq_defs: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("dq_defs"),
        "stdout mismatch for dq_defs"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16, so the recorded expectation
/// is nsc's tree and not merely ours.
#[test]
fn dq_defs_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect), Some(scalac)) =
        (scala_library_jar(), scala_reflect_jar(), find_scalac())
    else {
        eprintln!("skip dq_defs scalac diff: scalac / jars not obtainable");
        return;
    };
    let ref_out = tmp_dir("dq_defs-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("dq_defs.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected dq_defs.scala: {}",
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
        "java Main (real-scalac build) failed for dq_defs: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout("dq_defs"),
        "recorded expectation for dq_defs does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// The definition forms reification still refuses, each named.
///
/// Every one of these is a place where the parser has normalised away
/// something nsc keeps -- braces around an empty body, a by-name or repeated
/// parameter's type, the `=` that separates procedure syntax from a result
/// type, a pattern definition -- or a flag that does not fit the parser's word
/// (`PRESUPER` is bit 37). Building anything at all would build a definition
/// nobody wrote.
#[test]
fn dq_defs_bad_names_every_form_it_cannot_build() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip dq_defs_bad: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("dq_defs_bad");
    let output = compile_reflect("dq_defs_bad", &out, &jar, &reflect);
    assert!(!output.status.success(), "expected dq_defs_bad to fail");
    let err = diagnostics(&output);
    for needle in [
        "a self type (`class C { self => ... }`) is not reified yet",
        "an early definition (`extends { val x = 1 } with T`) is not reified yet",
        "a qualified access modifier (`private[X]`) is not reified yet",
        "a by-name parameter is not reified yet",
        "a repeated parameter (`T*`) is not reified yet",
        "procedure syntax (`def f() { ... }`) is not reified yet",
        "a `def` with neither a result type nor a body is not reified yet",
        "a pattern definition (`val (a, b) = ...`) is not reified yet",
        "a higher-kinded type parameter is not reified yet",
        "a context bound (`T : C`) is not reified yet",
        "a `case` class whose parents are a `..$` splice is not reified yet",
        "an implicit parameter clause that is not the last is not reified yet",
        "a `macro` definition is not reified yet",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// --- `Liftable`: holes that are not trees (`agent/liftable`, §7.8) ---------

/// `tests/fixtures/lf2_lift.scala`, run.
///
/// A quasiquote hole does not have to be a `Tree`: nsc infers an implicit
/// `Liftable[T]` for the argument's type and splices `Liftable.liftX[T](arg)`
/// (`scala/reflect/api/StandardLiftables.scala`). scala-rs picks the standard
/// instance from the type and builds *the tree that instance builds*, so what
/// this pins is the tree, not that something typechecked: every line prints
/// `showRaw`, and `show` where the result is a `TypeTree` (whose `showRaw`
/// hides the type it carries).
///
/// `lf2_lift_matches_real_scalac` is what makes the recorded expectation mean
/// anything: without it this would only record what we happen to build.
#[test]
fn lf2_lift_builds_the_standard_liftable_trees() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip lf2_lift: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("lf2_lift");
    let output = compile_reflect("lf2_lift", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile lf2_lift failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}:{}", out.display(), reflect.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for lf2_lift: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("lf2_lift"),
        "stdout mismatch for lf2_lift"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16, whose own quasiquote macro
/// does the `Liftable` inference. Every instance in `crate::reify::Lift` is
/// pinned to the tree nsc's instance produces.
#[test]
fn lf2_lift_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect), Some(scalac)) =
        (scala_library_jar(), scala_reflect_jar(), find_scalac())
    else {
        eprintln!("skip lf2_lift scalac diff: scalac / jars not obtainable");
        return;
    };
    let ref_out = tmp_dir("lf2_lift-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("lf2_lift.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected lf2_lift.scala: {}",
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
        "java Main (real-scalac build) failed for lf2_lift: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout("lf2_lift"),
        "recorded expectation for lf2_lift does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// `tests/fixtures/lf2_ctx.scala`: the two instances only a macro
/// implementation can reach.
///
/// A `WeakTypeTag` arrives in the implicit clause a macro's type parameters
/// come through, and an `Expr` is what `c.prefix` is -- neither can be got at
/// run time without a materialiser, so they are checked here by compiling the
/// implementation itself, the way `qq_ctx.scala` is. This is the shape slick's
/// `ShapedValue.mapToImpl` is written in: `q"($rModule.tupled) : ($uTag =>
/// $rTag)"` reported `no matching overload for SyntacticFunctionTypeExtractor
/// with arguments (List[TypeTags$WeakTypeTag[U]], TypeTags$WeakTypeTag[R])`
/// before this.
///
/// It also covers `symbolOf[T]` / `weakTypeOf[T]`, whose type parameter is
/// named only in their implicit clause -- `pin_undetermined_tparams` used to
/// refuse the member outright, so `symbolOf` was "not found: value symbolOf".
#[test]
fn lf2_ctx_lifts_tags_and_exprs_in_a_macro_implementation() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip lf2_ctx: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("lf2_ctx");
    let output = compile_reflect("lf2_ctx", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile lf2_ctx failed: {}",
        diagnostics(&output)
    );

    if java_available() {
        // Loading the class links and verifies it, which is the check
        // available here: expanding a macro needs the JVM bridge
        // (`docs/macros.md` §2.2), which is not built.
        let loader = out.join("loader");
        fs::create_dir_all(&loader).unwrap();
        let src = out.join("Loader.scala");
        fs::write(
            &src,
            "object Main {\n  \
             def main(args: Array[String]): Unit = println(Class.forName(\"Lf2Ctx$\").getName)\n\
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
        assert_eq!(String::from_utf8_lossy(&run.stdout), "Lf2Ctx$\n");
    }

    let Some(scalac) = find_scalac() else {
        eprintln!("skip the scalac half of lf2_ctx: scalac 2.13 not obtainable");
        let _ = fs::remove_dir_all(&out);
        return;
    };
    let ref_out = tmp_dir("lf2_ctx-scalac");
    let built = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("lf2_ctx.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        built.status.success(),
        "real scalac rejected lf2_ctx.scala: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&out);
}

/// `tests/fixtures/lf3_identsym.scala`: `u.Ident(sym: Symbol)`, the overload
/// `PickleSupply::erased_param_desc` used to drop entirely.
///
/// `Ident` is both `val Ident: IdentExtractor` (the tree factory, `apply
/// (Name)`) *and* a separate convenience method `def Ident(sym: Symbol):
/// Ident` declared directly on `scala.reflect.internal.Trees` -- confirmed
/// with `javap` against scala-reflect.jar 2.13.16, which shows
/// `scala/reflect/api/Trees.class` declaring `abstract Trees$IdentApi Ident
/// (Symbols$SymbolApi)` right next to the extractor. `erased_param_desc` had
/// no case for `Type::TypeMember` -- what an abstract type member like
/// `Symbol` converts to when it is reached from the abstract API rather than
/// the concrete `JavaUniverse` a macro only gets at expansion time -- and
/// fell through to the "any reference slot" wildcard, indistinguishable from
/// `Ident(String)`'s own reference parameter. slick's
/// `TableQueryMacroImpl.apply` is written in `Ident(typeOf[Tag].typeSymbol)`.
#[test]
fn lf3_identsym_supplies_the_symbol_overload_of_ident() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip lf3_identsym: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("lf3_identsym");
    let output = compile_reflect("lf3_identsym", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile lf3_identsym failed: {}",
        diagnostics(&output)
    );

    if java_available() {
        let loader = out.join("loader");
        fs::create_dir_all(&loader).unwrap();
        let src = out.join("Loader.scala");
        fs::write(
            &src,
            "object Main {\n  \
             def main(args: Array[String]): Unit = println(Class.forName(\"Lf3IdentSym$\").getName)\n\
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
        assert_eq!(String::from_utf8_lossy(&run.stdout), "Lf3IdentSym$\n");
    }

    let Some(scalac) = find_scalac() else {
        eprintln!("skip the scalac half of lf3_identsym: scalac 2.13 not obtainable");
        let _ = fs::remove_dir_all(&out);
        return;
    };
    let ref_out = tmp_dir("lf3_identsym-scalac");
    let built = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("lf3_identsym.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        built.status.success(),
        "real scalac rejected lf3_identsym.scala: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&out);
}

/// What `Liftable` refuses, and `reify { … }`, each named.
///
/// The failure that would matter is the quiet one: lifting a type scala-rs has
/// no instance for would build a tree nobody wrote. And `reify` is a
/// compiler-internal macro like the quasiquotes -- reporting `value reify is
/// not a member of JavaUniverse` was the same untruth `value q is not a member
/// of StringContext` was, and points at the wrong thing to fix.
#[test]
fn lf2_lift_bad_names_every_hole_it_cannot_lift() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip lf2_lift_bad: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("lf2_lift_bad");
    let output = compile_reflect("lf2_lift_bad", &out, &jar, &reflect);
    assert!(!output.status.success(), "expected lf2_lift_bad to fail");
    let err = diagnostics(&output);
    for needle in [
        // No standard instance for this type.
        "a hole of type `File` is not lifted",
        // nsc's `liftList` builds a `List(...)` call, not a splice; scala-rs
        // does not build it, and does not approximate it either.
        "a hole of type `List[Int]` is not lifted",
        // A `Symbol` is lifted on its own but has no `Liftable`, so `..$` over
        // symbols is refused -- nsc refuses it too.
        "a hole of type `Symbols.ModuleSymbol` is not lifted",
        "macro expansion is not implemented: cannot expand reify { ... }",
        "docs/macros.md",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    // The old, untrue diagnostic must not come back.
    assert!(
        !err.contains("value reify is not a member"),
        "`reify` reported as a missing member again: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

// --- the fresh-name forms (`agent/freshname`, §7.10) ----------------------

/// Renumber the fresh names in one line of `showRaw` output, in order of first
/// appearance.
///
/// The three forms in `tests/fixtures/fn2_fresh.scala` get their names from
/// the *universe's* `freshTermName` / `freshTypeName` at run time, off one
/// global counter, so `x$7` says nothing except "the seventh name this JVM
/// handed out". Two things make the raw numbers differ between the two
/// compilers even when the trees are identical: the counter is shared with
/// every line before this one, and nsc happens to allocate right-to-left
/// (`q"_.foo(_)"` names the argument's parameter before the receiver's).
///
/// Renumbering per line from 1, in order of first appearance, drops exactly
/// that and keeps everything else -- in particular **which binder each
/// occurrence refers to**, which is the whole point of these forms: `_$1 ...
/// _$2` and `_$1 ... _$1` do not normalise to the same string, so a
/// reification that reused one name where nsc binds two still fails.
///
/// A fresh name is `<prefix>$<digits>` (`x$1`, `_$1`, `rassoc$1`).
/// `$colon$colon` and `x$pf` have no digits after the `$` and are left alone.
fn renumber_fresh_names(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        let mut seen: Vec<&str> = Vec::new();
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'$' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
                // Walk back over the prefix and forward over the digits.
                let mut lo = i;
                while lo > 0 && (b[lo - 1].is_ascii_alphanumeric() || b[lo - 1] == b'_') {
                    lo -= 1;
                }
                let mut hi = i + 1;
                while hi < b.len() && b[hi].is_ascii_digit() {
                    hi += 1;
                }
                if lo < i {
                    let name = &line[lo..hi];
                    let n = match seen.iter().position(|s| *s == name) {
                        Some(k) => k,
                        None => {
                            seen.push(name);
                            seen.len() - 1
                        }
                    };
                    // The prefix is already in `out` -- everything up to `i`
                    // was copied byte by byte -- so only the number changes.
                    out.push('$');
                    out.push_str(&(n + 1).to_string());
                    i = hi;
                    continue;
                }
            }
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

/// `tests/fixtures/fn2_fresh.scala`, run: the three forms whose expansion nsc
/// builds out of a fresh name.
///
/// A `_` placeholder lambda, a `_` type argument (an existential) and a
/// right-associative operator are the forms where nsc's expansion is a *block*
/// -- `val nn$macro$k = u.internal.reificationSupport.freshTermName("x$")`
/// ahead of the call -- and not one expression. Getting the block wrong is
/// invisible to a compile: the tree would simply carry a different name, or
/// the same name in two places that have to differ. So this runs the fixture
/// and compares `showRaw` (renumbered, see `renumber_fresh_names`).
#[test]
fn fn2_fresh_reifies_the_fresh_name_forms() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip fn2_fresh: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("fn2_fresh");
    let output = compile_reflect("fn2_fresh", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile fn2_fresh failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}:{}", out.display(), reflect.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for fn2_fresh: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        renumber_fresh_names(&String::from_utf8_lossy(&run.stdout)),
        renumber_fresh_names(&expected_stdout("fn2_fresh")),
        "stdout mismatch for fn2_fresh"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16. Without this the recorded
/// expectation would only say what we happen to build; with it, every fresh
/// name -- how many, and where each one is used -- is pinned to nsc's own
/// quasiquote expansion.
#[test]
fn fn2_fresh_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect), Some(scalac)) =
        (scala_library_jar(), scala_reflect_jar(), find_scalac())
    else {
        eprintln!("skip fn2_fresh scalac diff: scalac / jars not obtainable");
        return;
    };
    let ref_out = tmp_dir("fn2_fresh-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("fn2_fresh.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected fn2_fresh.scala: {}",
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
        "java Main (real-scalac build) failed for fn2_fresh: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        renumber_fresh_names(&String::from_utf8_lossy(&reference.stdout)),
        renumber_fresh_names(&expected_stdout("fn2_fresh")),
        "recorded expectation for fn2_fresh does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// A `_` with nothing to bind it is still refused, in both name spaces.
///
/// `q"_"` and `tq"_"` are errors in real scalac too ("unbound placeholder
/// parameter", "unbound wildcard type"), so this is the same answer and not a
/// gap: a `_` is a name only relative to the lambda or the applied type that
/// binds it.
#[test]
fn fn2_fresh_bad_refuses_an_unbound_wildcard() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip fn2_fresh_bad: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("fn2_fresh_bad");
    let output = compile_reflect("fn2_fresh_bad", &out, &jar, &reflect);
    assert!(!output.status.success(), "expected fn2_fresh_bad to fail");
    let err = diagnostics(&output);
    for needle in [
        "quasiquote q\"...\" (unbound placeholder parameter)",
        "quasiquote tq\"...\" (a `_` type argument (an existential) is not reified yet)",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The renumbering itself: the comparison above is only as good as this is.
#[test]
fn renumber_fresh_names_keeps_binder_identity() {
    // Order of first appearance, per line, from 1.
    assert_eq!(
        renumber_fresh_names("f(x$7, x$5, x$7)\n"),
        "f(x$1, x$2, x$1)\n"
    );
    // Two lines do not share a numbering.
    assert_eq!(renumber_fresh_names("a(_$4)\nb(_$9)\n"), "a(_$1)\nb(_$1)\n");
    // Two distinct binders never collapse into one.
    assert_ne!(
        renumber_fresh_names("P(_$6, _$5)"),
        renumber_fresh_names("P(_$6, _$6)")
    );
    // Names with no number after the `$` are left alone.
    assert_eq!(
        renumber_fresh_names("Select(x$pf, $colon$colon)"),
        "Select(x$pf, $colon$colon)"
    );
}

/// `tests/fixtures/tt_tags.scala`: `TypeTag` / `WeakTypeTag` materialisation.
///
/// Nothing in that file writes a tag down. `typeOf[T]` asks for an implicit
/// `TypeTag[T]`, and nsc answers it not by *finding* a value but by expanding
/// the compiler-internal `materializeTypeTag[T](u)` into a `TypeCreator` that
/// rebuilds `T` inside the mirror the tag is handed; scala-rs has to do the
/// same or `c.typeOf[HList]` and `implicitly[TypeTag[T]]` are dead ends
/// (`crates/typer/src/materialize.rs`, `docs/macros.md` §7.10).
///
/// The tree scala-rs builds is *not* nsc's tree -- it skips the `$u` / `$m`
/// bindings, casts the mirror, and writes the creator's erased result type
/// out -- so what is pinned here is the answer: 30 lines of `tag.tpe`
/// printed, `=:=` and `<:<` between independently materialised tags, and the
/// symbol each tag names.
#[test]
fn tt_tags_materialises_type_tags() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip tt_tags: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("tt_tags");
    let output = compile_reflect("tt_tags", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile tt_tags failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}:{}", out.display(), reflect.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for tt_tags: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("tt_tags"),
        "stdout mismatch for tt_tags"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16, whose own
/// `materializeTypeTag` builds the tags. Trees differ; every answer must not.
#[test]
fn tt_tags_matches_real_scalac() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect), Some(scalac)) =
        (scala_library_jar(), scala_reflect_jar(), find_scalac())
    else {
        eprintln!("skip tt_tags scalac diff: scalac / jars not obtainable");
        return;
    };
    let ref_out = tmp_dir("tt_tags-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("tt_tags.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected tt_tags.scala: {}",
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
        "java Main (real-scalac build) failed for tt_tags: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout("tt_tags"),
        "recorded expectation for tt_tags does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// `tests/fixtures/tt_ctx.scala`: the same materialisation inside a macro
/// *implementation*, where the universe is `c.universe` and the mirror is its
/// `rootMirror`.
///
/// This is slick's `ShapedValue.mapToImpl` shape --
/// `uTag.tpe <:< c.typeOf[HList]` -- which reported "no implicit: could not
/// find implicit value of type TypeTags$TypeTag[HList]" before. Expanding a
/// macro needs the JVM bridge (`docs/macros.md` §2.2), so what is checked is
/// that both compilers accept the file and that the class file loads and
/// verifies.
#[test]
fn tt_ctx_materialises_in_a_macro_implementation() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip tt_ctx: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("tt_ctx");
    let output = compile_reflect("tt_ctx", &out, &jar, &reflect);
    assert!(
        output.status.success(),
        "compile tt_ctx failed: {}",
        diagnostics(&output)
    );

    if java_available() {
        let loader = out.join("loader");
        fs::create_dir_all(&loader).unwrap();
        let src = out.join("Loader.scala");
        fs::write(
            &src,
            "object Main {\n  \
             def main(args: Array[String]): Unit = println(Class.forName(\"TtCtx$\").getName)\n\
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
        assert_eq!(String::from_utf8_lossy(&run.stdout), "TtCtx$\n");
    }

    let Some(scalac) = find_scalac() else {
        eprintln!("skip the scalac half of tt_ctx: scalac 2.13 not obtainable");
        let _ = fs::remove_dir_all(&out);
        return;
    };
    let ref_out = tmp_dir("tt_ctx-scalac");
    let built = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("tt_ctx.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        built.status.success(),
        "real scalac rejected tt_ctx.scala: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&out);
}

/// What the materialiser refuses, each named.
///
/// The failure that would matter is the quiet one: a tag built for a type
/// that is not the type asked about is not a compile error at all, it is a
/// wrong `Type` handed to a macro at run time. So a shape one `staticClass`
/// call cannot rebuild says which shape it was, and the old "could not find
/// implicit value of type TypeTag[List[Int]]" -- which pointed at a value no
/// program was ever going to define -- must not come back for these.
#[test]
fn tt_tags_bad_names_every_tag_it_cannot_build() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip tt_tags_bad: scala-library / scala-reflect not obtainable");
        return;
    };
    let out = tmp_dir("tt_tags_bad");
    let output = compile_reflect("tt_tags_bad", &out, &jar, &reflect);
    assert!(!output.status.success(), "expected tt_tags_bad to fail");
    let err = diagnostics(&output);
    for needle in [
        // A constructor at its arguments is built now (`docs/macros.md`
        // §7.12) -- `tt_tags.scala` runs those against real scalac. What is
        // refused is a constructor whose *argument* has no body, and a shape
        // with no `staticClass` at all.
        "cannot build a WeakTypeTag for `(Int, Foo)`, whose type arguments would have to be \
         reified too",
        "cannot build a TypeTag for `(Int) => Foo`, whose type arguments would have to be \
         reified too",
        "cannot build a TypeTag for `Inner`, a class nested in a class or an object",
        "cannot build a TypeTag for `AnyRef`, which is an alias rather than a class",
        "cannot build a TypeTag for `T`, an abstract type with no tag in scope",
        "cannot build a WeakTypeTag for `T`, an abstract type with no tag in scope",
        "cannot build a TypeTag for `Main.type`, a singleton type",
        "docs/macros.md",
    ] {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics, got {err}"
        );
    }
    assert!(
        !err.contains("could not find implicit value of type TypeTags$TypeTag"),
        "a tag request reported as a plain missing implicit again: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
