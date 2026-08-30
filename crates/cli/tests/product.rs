//! E2E tests for the `agent/product` slice: a `case class` / `case object` as
//! a real `scala.Product`, its four overridden accessors (`productPrefix`,
//! `productArity`, `productElement`, `productElementName`), the two it
//! inherits (`productIterator`, `productElementNames`), and the synthetic
//! companion as a `scala.runtime.AbstractFunctionN` -- which is where `tupled`
//! and `curried` come from.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new fixtures
//! use the `prod` prefix.
//!
//! `crates/typer/src/prelude_product.rs` records what was read off scalac's
//! own classfiles with `javap -v -p`; the `real_scalac_*` tests below keep the
//! recorded expectations honest by running the same fixtures through scalac
//! 2.13.16 and diffing stdout.

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
        "scala-rs-prod-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Private-runtime check (`--no-scala-library`).
fn check_private_runtime(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for private-runtime {name}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Library-ABI check (`--scala-library`).
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded expectation
/// and scalac's own stdout have to agree, and so does ours. Everything this
/// slice adds is a *synthetic* member whose exact behaviour (which index
/// throws, with which message, and what `productIterator` yields) is only
/// pinned down by the real compiler.
fn real_scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff {name}: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference = String::from_utf8_lossy(&reference.stdout).to_string();
    assert_eq!(
        reference,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, jar.to_str().unwrap().into()),
        reference,
        "stdout differs from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
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
        "expected compile of {name} (extra={extra:?}) to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Reads a class file's constant-pool-resolved shape via `javap`.
fn javap(out: &Path, class: &str) -> String {
    let text = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    String::from_utf8_lossy(&text.stdout).into_owned()
}

/// The four accessors nsc overrides, the two it inherits, and the value-class
/// arm, in both library modes.
#[test]
fn fixtures_prod_private_runtime() {
    check_private_runtime("prod");
}

#[test]
fn fixtures_prod_library() {
    dual_run_fixture("prod");
}

#[test]
fn real_scalac_dual_run_prod() {
    real_scalac_dual_run("prod");
}

#[test]
fn fixtures_prod_vc_private_runtime() {
    check_private_runtime("prod_vc");
}

#[test]
fn fixtures_prod_vc_library() {
    dual_run_fixture("prod_vc");
}

#[test]
fn real_scalac_dual_run_prod_vc() {
    real_scalac_dual_run("prod_vc");
}

/// `Product` as a *type*, `productIterator` / `productElementNames`, and
/// `tupled` / `curried` on the companion -- all of which need the real jar.
#[test]
fn fixtures_prod_lib_library() {
    dual_run_fixture("prod_lib");
}

#[test]
fn real_scalac_dual_run_prod_lib() {
    real_scalac_dual_run("prod_lib");
}

/// The private runtime has no `scala/Product`, no `scala/collection/Iterator`
/// and no `scala/runtime/AbstractFunctionN`, so `--no-scala-library` must keep
/// diagnosing every one of those -- not quietly accept a call the backend
/// could not emit.
#[test]
fn fixtures_prod_lib_without_library_is_error() {
    compile_fails(
        "prod_lib",
        &["--no-scala-library"],
        &[
            "value productIterator is not a member of P",
            "value productElementNames is not a member of P",
            "value tupled is not a member of P$",
            "value curried is not a member of P$",
        ],
    );
}

/// `Product` is a case-class thing: a plain class has none of it, the index is
/// an `Int`, and a plain class does not conform to `Product`. scalac rejects
/// all four of these too.
#[test]
fn fixtures_prod_bad() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip prod_bad: jar not obtainable");
        return;
    };
    compile_fails(
        "prod_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "value productArity is not a member of Plain",
            "value productElement is not a member of Plain",
            "no matching overload for (Int)Any with arguments (\"0\")",
            "type mismatch; found: Plain  required: Product",
        ],
    );
}

/// The emitted shape, not just the behaviour: nsc's
/// `class Main$P implements scala.Product, java.io.Serializable` and
/// `class Main$P$ extends scala.runtime.AbstractFunction2 implements
/// java.io.Serializable`, with `productIterator` delegating to
/// `ScalaRunTime$.typedProductIterator` and `productElementNames` to
/// `Product.productElementNames$` rather than being open-coded.
#[test]
fn prod_lib_classfile_shape() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip prod_lib_classfile_shape: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("prod_lib", &["--scala-library", jar.to_str().unwrap()]);

    let p = javap(&out, "Main$P");
    for needle in [
        "implements scala.Product,java.io.Serializable",
        "public java.lang.Object productElement(int);",
        "public java.lang.String productElementName(int);",
        "tableswitch",
        "scala/runtime/Statics.ioobe:(I)Ljava/lang/Object;",
        "scala/runtime/ScalaRunTime$.typedProductIterator:(Lscala/Product;)Lscala/collection/Iterator;",
        "scala/Product.productElementNames$:(Lscala/Product;)Lscala/collection/Iterator;",
    ] {
        assert!(needle_in(&p, needle), "Main$P missing {needle:?}:\n{p}");
    }

    let comp = javap(&out, "Main$P$");
    for needle in [
        "extends scala.runtime.AbstractFunction2",
        "implements java.io.Serializable",
        // The erased `FunctionN.apply` bridge, without which the class does
        // not implement its own superclass.
        "public java.lang.Object apply(java.lang.Object, java.lang.Object);",
    ] {
        assert!(
            needle_in(&comp, needle),
            "Main$P$ missing {needle:?}:\n{comp}"
        );
    }

    // Arity 22 is the last `AbstractFunctionN`; a `case object` is a `Product`
    // but never an `AbstractFunctionN`.
    let big = javap(&out, "Main$Big22$");
    assert!(
        needle_in(&big, "extends scala.runtime.AbstractFunction22"),
        "Main$Big22$ should extend AbstractFunction22:\n{big}"
    );
    let solo = javap(&out, "Main$Solo$");
    assert!(
        needle_in(&solo, "implements scala.Product,java.io.Serializable"),
        "Main$Solo$ should be a Product:\n{solo}"
    );
    assert!(
        !needle_in(&solo, "AbstractFunction"),
        "Main$Solo$ must not extend AbstractFunctionN:\n{solo}"
    );
    // A `case object` inherits `productElementName` rather than overriding it
    // (nsc synthesizes none), so the module class carries the mixin forwarder.
    assert!(
        needle_in(
            &solo,
            "scala/Product.productElementName$:(Lscala/Product;I)Ljava/lang/String;"
        ),
        "Main$Solo$ should forward productElementName to Product's default:\n{solo}"
    );

    let _ = fs::remove_dir_all(&out);
}

fn needle_in(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}
