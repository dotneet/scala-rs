//! `trait T extends C` — a trait whose parent is a class (SLS 5.3.3) — and the
//! `abstract override` / stackable-trait rules that go with it.
//!
//! Everything asserted here was read off real scalac 2.13.16 first: the
//! expected stdout files are its own output for the same fixtures, and every
//! diagnostic string is its wording, verbatim (`javap -v -p` on
//! `Main$Loud` / `Main$$anon$N` supplied the shape of the super accessors).
//!
//! Kept in its own file so it does not collide with the parallel work landing
//! in `e2e.rs`.

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
        "scala-rs-trex-{tag}-{}-{nanos}-{seq}",
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
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
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
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Private-runtime mode: `--no-scala-library`.
fn check_private(name: &str) {
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

/// Library-ABI mode: linked against the real scala-library 2.13.16 jar. The
/// expected file is real scalac's own output for the same source.
fn check_library(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn diagnostics(name: &str, extra: &[&str]) -> String {
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
    let _ = fs::remove_dir_all(&out);
    err
}

/// Both modes must reject it: a diagnostic that only fires with the jar on the
/// classpath would let the private runtime miscompile in silence.
fn compile_fails_both(name: &str, needles: &[&str]) {
    let mut modes: Vec<Vec<String>> = vec![vec!["--no-scala-library".to_string()]];
    if let Some(jar) = scala_library_jar() {
        modes.push(vec![
            "--scala-library".to_string(),
            jar.to_str().unwrap().to_string(),
        ]);
    }
    for m in &modes {
        let args: Vec<&str> = m.iter().map(|s| s.as_str()).collect();
        let err = diagnostics(name, &args);
        for needle in needles {
            assert!(
                err.contains(needle),
                "expected {needle:?} in diagnostics for {name} ({args:?}), got {err:?}"
            );
        }
    }
}

fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(
        output.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------- happy path

/// The report that started this: `trait Loud extends Animal` where `Animal`
/// takes a constructor parameter. The trait never runs that constructor, so
/// no argument list is resolved for it.
#[test]
fn fixtures_trex_stack_private_runtime() {
    check_private("trex_stack");
}

#[test]
fn fixtures_trex_stack_scala_library() {
    check_library("trex_stack");
}

/// `class X extends Loud` acquires `Loud`'s superclass as its own (SLS 5.1),
/// on the JVM as well as in the type system.
#[test]
fn fixtures_trex_inherit_private_runtime() {
    check_private("trex_inherit");
}

#[test]
fn fixtures_trex_inherit_scala_library() {
    check_library("trex_inherit");
}

/// scalac 2.13.16 gives the trait an abstract super accessor and grounds it in
/// the concrete class: `Main$$anon$1.Loud$$super$speak` is
/// `invokespecial Main$Dog.speak`, and `speak` itself forwards to the trait's
/// implementation. Check the same two shapes here.
#[test]
fn trex_super_accessor_shape() {
    let out = compile_fixture_with("trex_stack", &["--no-scala-library"]);
    let anon = javap(&out, "Main$$anon$1");
    assert!(
        anon.contains("Loud$$super$speak"),
        "no super accessor in the anonymous class: {anon}"
    );
    assert!(
        anon.contains("invokespecial") && anon.contains("Main$Dog.speak"),
        "the super accessor must reach the superclass statically: {anon}"
    );
    // The class that mixes the trait in also carries the JVM superclass.
    let loud_dog = javap(&out, "Main$LoudDog");
    assert!(
        loud_dog.contains("extends Main$Dog"),
        "LoudDog must extend Dog: {loud_dog}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `class X extends Loud` must extend `Main$Animal` in the class file, not
/// `java/lang/Object` — otherwise `val a: Animal = new X` fails the verifier.
#[test]
fn trex_inherited_superclass_reaches_the_class_file() {
    let out = compile_fixture_with("trex_inherit", &["--no-scala-library"]);
    let x = javap(&out, "Main$X");
    assert!(
        x.contains("extends Main$Animal"),
        "X must extend Animal: {x}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The trait's own interface must not claim the superclass: scalac emits
/// `public interface Main$Loud` with `java/lang/Object` as its super, and a
/// trait body reaching an inherited member has to cast `$this` first.
#[test]
fn trex_trait_interface_does_not_extend_its_superclass() {
    let out = compile_fixture_with("trex_stack", &["--no-scala-library"]);
    let loud = javap(&out, "Main$Loud");
    assert!(
        loud.contains("interface Main$Loud"),
        "Loud must be an interface: {loud}"
    );
    assert!(
        !loud.contains("extends Main$Animal"),
        "a trait's class parent is a constraint, not a JVM supertype: {loud}"
    );
    let impl_cls = javap(&out, "Main$Loud$class");
    assert!(
        impl_cls.contains("checkcast") && impl_cls.contains("Main$Animal"),
        "the trait body must checkcast $this before reading an inherited member: {impl_cls}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ bad path

/// SLS 5.3.3: only a subclass of the trait's superclass may mix it in. Both
/// the named class and the anonymous one are rejected, once each — scalac
/// 2.13.16 reports exactly the same two.
#[test]
fn fixtures_trex_mixin_bad_is_error() {
    compile_fails_both(
        "trex_mixin_bad",
        &[
            "illegal inheritance; superclass Plain",
            "is not a subclass of the superclass Animal",
            "of the mixin trait Loud",
        ],
    );
    let err = diagnostics("trex_mixin_bad", &["--no-scala-library"]);
    assert_eq!(
        err.matches("illegal inheritance; superclass Plain").count(),
        2,
        "one diagnostic per offending template, not one per typer pass: {err}"
    );
}

/// `abstract override` that never reaches a concrete implementation. Before
/// this check the backend emitted a `throw new RuntimeException` stub for the
/// super accessor and the program failed at run time.
#[test]
fn fixtures_trex_ungrounded_bad_is_error() {
    compile_fails_both(
        "trex_ungrounded_bad",
        &[
            "object creation impossible.",
            "is marked `abstract` and `override`, but no concrete implementation could be found in a base class",
        ],
    );
}

/// A trait's parent takes no argument list.
#[test]
fn fixtures_trex_ctorargs_bad_is_error() {
    compile_fails_both(
        "trex_ctorargs_bad",
        &["parents of traits may not have parameters"],
    );
}

/// `abstract override` outside a trait has no linearized `super` to name.
#[test]
fn fixtures_trex_absover_class_bad_is_error() {
    compile_fails_both(
        "trex_absover_class_bad",
        &["`abstract override` modifier only allowed for members of traits"],
    );
}

/// An `object` is an instance too, and scalac words it the same way.
#[test]
fn fixtures_trex_object_bad_is_error() {
    compile_fails_both(
        "trex_object_bad",
        &[
            "object creation impossible.",
            "is marked `abstract` and `override`, but no concrete implementation could be found in a base class",
        ],
    );
}

/// The class's own implementation sits *above* the trait in the
/// linearization, so it cannot ground the trait's `super`.
#[test]
fn fixtures_trex_ownimpl_bad_is_error() {
    compile_fails_both(
        "trex_ownimpl_bad",
        &[
            "`abstract override` modifiers required to override:",
            "abstract override def speak: String (defined in trait Loud)",
        ],
    );
}
