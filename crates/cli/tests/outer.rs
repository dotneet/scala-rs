//! `$outer` for member classes of a *trait*, and the erasure bridge a module
//! needs when it overrides with a narrower result type.
//!
//! Every runtime check here is a dual-run: the fixture's expected output is
//! what real scalac 2.13.16 prints, and the classfiles are verified with
//! `java -Xverify:all` against the real `scala-library` jar as well as against
//! the private runtime.

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

/// The counter matters: `SystemTime` is only microsecond-grained here, so two
/// tests entering this at the same instant would otherwise share an output
/// directory and delete each other's class files.
static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-outer-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
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

/// Run the fixture in both ABIs and compare against scalac's own output.
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

/// Trait member classes, two levels of nesting, a mixed-in class and a
/// mixed-in object as the enclosing instance, and `new prefix.Inner`.
#[test]
fn fixtures_outer() {
    check_both_abis("outer");
}

/// A covariant (narrower) result type in an override, including from a
/// `case object`.
#[test]
fn fixtures_outer_bridge() {
    check_both_abis("outer_bridge");
}

/// slick's cake shape: a component trait whose member class reaches the *self
/// type*'s members, plus a local class in a trait method and an anonymous
/// class inside a trait's member class.
#[test]
fn fixtures_outer_self() {
    check_both_abis("outer_self");
}

/// nsc types `$outer` as the enclosing trait's self type when that self type
/// is a subclass of the trait: `trait Comp { self: Prof => class Table }`
/// stores a `Prof`, so `Table` reaches `Prof`'s members with no cast.
#[test]
fn outer_field_is_the_self_type() {
    let out = compile("outer_self", "selfshape", &["--no-scala-library"]);
    let table = fs::read(out.join("Comp$Table.class")).expect("Comp$Table.class");
    assert!(
        contains(&table, b"(LProf;Ljava/lang/String;)V"),
        "Comp$Table's `$outer` must be the self type Prof, not Comp"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The `$outer` a trait's member class carries must be the trait's *interface*
/// type and the constructor's first parameter — that is what nsc emits, and
/// what lets a class mixing the trait in hand `this` to `new Inner`.
#[test]
fn outer_field_is_the_trait_interface() {
    let out = compile("outer", "shape", &["--no-scala-library"]);
    let inner = fs::read(out.join("T$Inner.class")).expect("T$Inner.class");
    let deep = fs::read(out.join("T$Inner$Deep.class")).expect("T$Inner$Deep.class");
    assert!(
        contains(&inner, b"$outer") && contains(&inner, b"(LT;Ljava/lang/String;)V"),
        "T$Inner must take its enclosing T first: `(LT;Ljava/lang/String;)V`"
    );
    assert!(
        contains(&deep, b"(LT$Inner;)V"),
        "T$Inner$Deep must take its enclosing T$Inner first"
    );
    let _ = fs::remove_dir_all(&out);
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
