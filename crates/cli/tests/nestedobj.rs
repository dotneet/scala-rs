//! A member `object` of a class or trait is one instance per enclosing
//! instance, not a static singleton: it carries an `$outer`, takes the
//! enclosing instance in its constructor, and the enclosing template hands it
//! out through a lazily initialised `<name>()` accessor.
//!
//! Every runtime check here is a dual-run: the fixture's expected output is
//! what real scalac 2.13.16 prints, and the classfiles are verified with
//! `java -Xverify:all` against the real `scala-library` jar as well as against
//! the private runtime. The shape assertions were read off
//! `javap -v -p -c` of scalac's own output for the same sources.

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
        "scala-rs-nestedobj-{tag}-{}-{nanos}-{n}",
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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Member `object`s of a class: the enclosing `val`, `Outer.this`, mutual
/// references between two of them, an `object` extending a member trait, an
/// `object` inside a non-static `object`, and identity — `o.P` twice is the
/// same instance, and two `Outer`s have different ones.
#[test]
fn fixtures_nestedobj() {
    check_both_abis("nestedobj");
}

/// Member `object`s of a *trait*, reached through an implementing class and
/// through an anonymous one, plus a `case class` nested in a class.
#[test]
fn fixtures_nestedobj_trait() {
    check_both_abis("nestedobj_trait");
}

/// scalac's shape: the object class holds a `private final $outer` and takes
/// the enclosing instance in its constructor, and has no `MODULE$` at all.
#[test]
fn member_object_takes_its_enclosing_instance() {
    let out = compile("nestedobj", "shape", &["--no-scala-library"]);
    let p = fs::read(out.join("Main$Outer$P$.class")).expect("Main$Outer$P$.class");
    assert!(
        contains(&p, b"$outer") && contains(&p, b"(LMain$Outer;)V"),
        "Main$Outer$P$ must take its enclosing Main$Outer: `(LMain$Outer;)V`"
    );
    assert!(
        !contains(&p, b"MODULE$"),
        "a member object is not a static singleton: no MODULE$ on Main$Outer$P$"
    );
    // `object N { object Deep }`: `N` is not static, so neither is `Deep`.
    let deep = fs::read(out.join("Main$Outer$N$Deep$.class")).expect("Main$Outer$N$Deep$.class");
    assert!(
        contains(&deep, b"(LMain$Outer$N$;)V"),
        "Main$Outer$N$Deep$ must take its enclosing Main$Outer$N$"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The enclosing class keeps the instance in a `<name>$module` field and
/// hands it out through a `<name>()` accessor, as nsc's mixin phase emits.
#[test]
fn enclosing_class_holds_the_module_field() {
    let out = compile("nestedobj", "field", &["--no-scala-library"]);
    let outer = fs::read(out.join("Main$Outer.class")).expect("Main$Outer.class");
    for needle in [
        &b"P$module"[..],
        &b"Q$module"[..],
        &b"()LMain$Outer$P$;"[..],
    ] {
        assert!(
            contains(&outer, needle),
            "Main$Outer must hold {:?}",
            String::from_utf8_lossy(needle)
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// A trait cannot hold a field, so the accessor is abstract on the interface
/// and the implementing class carries the field and the body. The trait also
/// reaches its own enclosing instance through `<Trait>$$$outer()`.
#[test]
fn trait_member_object_is_mixed_in() {
    let out = compile("nestedobj_trait", "trait", &["--no-scala-library"]);
    let iface = fs::read(out.join("Comp.class")).expect("Comp.class");
    assert!(
        contains(&iface, b"()LComp$Opt$;"),
        "the Comp interface must declare `Opt()LComp$Opt$;` abstractly"
    );
    let imp = fs::read(out.join("Impl.class")).expect("Impl.class");
    assert!(
        contains(&imp, b"Opt$module") && contains(&imp, b"()LComp$Opt$;"),
        "Impl must carry the field and the accessor for the trait's `Opt`"
    );
    let opt = fs::read(out.join("Comp$Opt$.class")).expect("Comp$Opt$.class");
    assert!(
        contains(&opt, b"(LComp;)V"),
        "Comp$Opt$ takes the trait interface as its enclosing instance"
    );

    // The trait nested in a class reaches the enclosing instance through the
    // expanded accessor, because an interface has no `$outer` field.
    let out2 = compile("nestedobj", "traitouter", &["--no-scala-library"]);
    let t = fs::read(out2.join("Main$Outer$T.class")).expect("Main$Outer$T.class");
    assert!(
        contains(&t, b"Main$Outer$T$$$outer"),
        "the inner trait must declare its expanded outer accessor"
    );
    let o = fs::read(out2.join("Main$Outer$O$.class")).expect("Main$Outer$O$.class");
    assert!(
        contains(&o, b"Main$Outer$T$$$outer"),
        "the object mixing the trait in must implement the outer accessor"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&out2);
}

/// A *local* `object` — one written inside a method — is a different shape
/// (nsc holds it in a per-call `scala.runtime.LazyRef`) and is not compiled
/// yet. It has to be a diagnostic, not a singleton that dies at run time.
/// An `object` inside a value class is rejected in scalac's own words.
#[test]
fn nested_object_bad_shapes_are_errors() {
    let text = diagnostics("nestedobj_bad");
    for needle in [
        "local `object`",
        "the enclosing instance",
        "implementation restriction: nested object is not allowed in value class",
    ] {
        assert!(
            text.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{text}"
        );
    }
}

/// A local `object` that reads nothing from outside is still fine.
#[test]
fn self_contained_local_object_still_compiles() {
    let out = compile("nestedobj", "selfcontained", &["--no-scala-library"]);
    let _ = fs::remove_dir_all(&out);
}
