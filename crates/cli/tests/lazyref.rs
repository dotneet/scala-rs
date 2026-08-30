//! Method-local `lazy val`s.
//!
//! nsc's `lazyvals` phase compiles a local `lazy val` into a
//! `scala.runtime.LazyRef` (or a monomorphic `LazyInt` / `LazyLong` / …) plus a
//! lifted accessor: the declaration only allocates the cell, and the
//! initialiser runs at the first *read*, at most once, behind the cell's
//! monitor. Before this, the initialiser ran at the declaration — the program
//! type-checked and produced the right values, it simply was not lazy, which
//! is exactly the kind of difference no diagnostic can catch.
//!
//! Every runtime check here is a dual-run against real scalac 2.13.16 output,
//! in both ABIs (`--scala-library` jar and the private runtime), verified with
//! `java -Xverify:all`. The shape assertions were read off `javap -p -c` of
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
        "scala-rs-lazyref-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
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
        "compile {name} ({tag}) failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
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

/// Run the fixture in both ABIs against the recorded expectation.
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

/// The recorded expectation is what real scalac 2.13.16 prints for the same
/// source: laziness is observable only in *when* the `println`s come out, so
/// the diff has to be against the real compiler, not against ourselves.
fn scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff {name}: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac"));
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
        "java Main (real-scalac build) failed for {name}: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Never forced, forced once, forced repeatedly; captured `val`, `var` and
/// method parameter; `lazy val`s reading each other in both directions; every
/// cell class; `Unit`; a fresh cell per loop iteration; inside and across a
/// lambda; an initialiser that throws and is retried; one inside a nested
/// `def`; and two with no result type written.
#[test]
fn fixtures_lr_local() {
    check_both_abis("lr_local");
}

#[test]
fn real_scalac_dual_run_lr_local() {
    scalac_dual_run("lr_local");
}

/// Read from a local class, a `return` out of the enclosing method (the
/// `return` moves into the accessor, so the method still has to carry the
/// `NonLocalReturnControl` handler), a value-class result, `match` and `try`
/// bodies, a cell read from an enclosing block, a trait method, a constructor
/// body, `this`, and two same-named `lazy val`s in sibling scopes.
#[test]
fn fixtures_lr_edge() {
    check_both_abis("lr_edge");
}

#[test]
fn real_scalac_dual_run_lr_edge() {
    scalac_dual_run("lr_edge");
}

/// A hoisted def calling another hoisted def has to be able to pass on its
/// captures. This was already broken for plain nested `def`s (the call came
/// out one argument short and the callee ran on a shifted frame); a local
/// `lazy val` inside a nested `def` is the same shape, since its accessor is
/// a hoisted def too.
#[test]
fn fixtures_lr_nestdef() {
    check_both_abis("lr_nestdef");
}

#[test]
fn real_scalac_dual_run_lr_nestdef() {
    scalac_dual_run("lr_nestdef");
}

/// The class / trait / object `lazy val` path (`bitmap$0` plus an accessor)
/// is untouched, including a template that has both a member and a local one.
#[test]
fn fixtures_lr_member() {
    check_both_abis("lr_member");
}

#[test]
fn real_scalac_dual_run_lr_member() {
    scalac_dual_run("lr_member");
}

/// A `lazy val` may be forward-referenced inside a block; an eager `val` may
/// not, and still is not.
#[test]
fn fixtures_lr_forward_bad_is_error() {
    let text = diagnostics("lr_forward_bad");
    assert!(
        text.contains("not found: value b"),
        "expected a not-found for the forward-referenced eager val:\n{text}"
    );
}

/// scalac's shape: the declaration allocates a cell and nothing else, the
/// reads go through a lifted accessor that takes the cell, and the unboxed
/// cell classes are used for primitives.
#[test]
fn local_lazy_val_compiles_to_a_lazy_cell() {
    let out = compile("lr_local", "shape", &["--no-scala-library"]);
    let m = fs::read(out.join("Main$.class")).expect("Main$.class");
    for needle in [
        &b"scala/runtime/LazyInt"[..],
        &b"scala/runtime/LazyLong"[..],
        &b"scala/runtime/LazyDouble"[..],
        &b"scala/runtime/LazyBoolean"[..],
        &b"scala/runtime/LazyRef"[..],
        &b"scala/runtime/LazyUnit"[..],
        &b"initialized"[..],
        &b"initialize"[..],
    ] {
        assert!(
            contains(&m, needle),
            "Main$ must reference {:?}",
            String::from_utf8_lossy(needle)
        );
    }
    // A local `lazy val` has no instance to hang a field on, so none of the
    // member machinery may appear for it.
    assert!(
        !contains(&m, b"bitmap$0"),
        "a method-local lazy val must not use the member `bitmap$0` scheme"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A member `lazy val` still gets the field + `bitmap$0` pair, and no cell.
#[test]
fn member_lazy_val_still_uses_the_bitmap() {
    let out = compile("lr_member", "shape", &["--no-scala-library"]);
    let b = fs::read(out.join("Box.class")).expect("Box.class");
    assert!(
        contains(&b, b"bitmap$0") && contains(&b, b"doubled"),
        "Box must keep the member `lazy val` field and its bitmap"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime carries its own cells; the library ABI must not emit
/// them, or they would shadow scala-library's own classes on the classpath.
#[test]
fn cells_come_from_the_private_runtime_only_when_it_is_used() {
    let out = compile("lr_local", "priv-cells", &["--no-scala-library"]);
    for rel in [
        "scala/runtime/LazyRef.class",
        "scala/runtime/LazyInt.class",
        "scala/runtime/LazyUnit.class",
    ] {
        assert!(
            out.join(rel).is_file(),
            "private runtime must emit {rel}, or a local lazy val cannot load"
        );
    }
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        return;
    };
    let out = compile(
        "lr_local",
        "lib-cells",
        &["--scala-library", jar.to_str().unwrap()],
    );
    for rel in [
        "scala/runtime/LazyRef.class",
        "scala/runtime/LazyInt.class",
        "scala/runtime/LazyUnit.class",
    ] {
        assert!(
            !out.join(rel).is_file(),
            "library ABI must not emit {rel} (it would collide with scala-library.jar)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Our cells have to answer the same three questions scala-library's do, with
/// the same descriptors, or a program compiled against one and run against the
/// other fails to link.
#[test]
fn private_cells_match_the_library_signatures() {
    if !java_available() {
        return;
    }
    let out = compile("lr_local", "cell-abi", &["--no-scala-library"]);
    let javap = Command::new("javap")
        .args([
            "-p",
            "-cp",
            out.to_str().unwrap(),
            "scala.runtime.LazyInt",
            "scala.runtime.LazyRef",
            "scala.runtime.LazyUnit",
        ])
        .output();
    let Ok(javap) = javap else {
        let _ = fs::remove_dir_all(&out);
        return;
    };
    if !javap.status.success() {
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let text = String::from_utf8_lossy(&javap.stdout).to_string();
    for needle in [
        "public boolean initialized();",
        "public int value();",
        "public int initialize(int);",
        "public java.lang.Object value();",
        "public java.lang.Object initialize(java.lang.Object);",
        "public void initialize();",
    ] {
        assert!(
            text.contains(needle),
            "private-runtime cell is missing `{needle}`:\n{text}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
