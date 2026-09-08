//! E2E tests for the `agent/triemapjava` slice.
//!
//! The slice started from the worst file in `tests/scalalib_measure.sh`:
//! `src/library/scala/collection/concurrent/TrieMap.scala` at 46 errors, a
//! Scala source whose classes extend the hand-written Java
//! `INodeBase`/`MainNode`/`CNodeBase`. The first question was whether the
//! measurement even sees the Java half. It does: `scalalib_measure.sh` runs
//! `--no-scala-library` with the 33 classfiles the library's 32 `.java`
//! sources compile to -- extracted from the released jar -- on `-cp`. So
//! nothing was held out and nothing needed to be; every error below is a real
//! compiler defect.
//!
//! Four of them, in the order they were peeled off:
//!
//! 1. **A written self-instantiating type-argument list was discarded.**
//!    `new C[K, V]` inside `class C[K, V]` writes arguments that *are* the
//!    instantiated class's own type-parameter symbols, and
//!    `check::type_args_are_instantiated` -- which decides whether the
//!    constructor path may believe `fun.ty`'s arguments -- rejects exactly
//!    that, because it cannot tell them from the placeholders an un-applied
//!    `new C` carries. The whole list was dropped (the concrete entries of a
//!    mixed `new C[K, Int]` with it) and re-inferred from the value
//!    arguments, which mention neither, so every parameter solved to
//!    `Nothing`. `CNode.updatedAt` / `removedAt` / `insertedAt` / `renewed`
//!    all end in `new CNode[K, V](…)` with no declared result type, so each
//!    got the inferred result `CNode[Nothing, Nothing]` and every caller
//!    failed: 13 of the file's 46 errors were the single overload
//!    `GCAS(cn, cn.renewed(startgen, ct), ct)`. **Nothing Java-specific about
//!    it** -- `tmj_selfctor.scala` reproduces it in five lines of plain
//!    Scala. Fixed by asking the *tree* whether the programmer wrote a type
//!    argument list (`check_apply::new_wrote_type_args`) instead of trying to
//!    read that off the type.
//!
//! 2. **Package-private Java members were dropped by the class-file reader.**
//!    `javaclass::java_member_visible` kept only `public` and `protected`.
//!    `INodeBase.java` declares `static final Object RESTART` and
//!    `NO_SUCH_ELEMENT_SENTINEL` with default access, and `class INode` reads
//!    them both as `INodeBase.RESTART` and through `import INodeBase._` -- 11
//!    more errors. Real scalac accepts all of those and rejects the same
//!    names from another package; it is the access check, not the reader,
//!    that draws the line. Now admitted and marked `PRIVATE` with
//!    `private_within = <package>`, which is Scala's `private[pkg]` and the
//!    same rule. (`Flags` is a full `u32`, hence the existing qualifier
//!    rather than a 33rd bit.)
//!
//! 3. **A field's `Signature` attribute was read and thrown away.** Methods
//!    have taken their generic signature since they were first loaded;
//!    fields took only the erased descriptor, so `INodeBase.java`'s `public
//!    volatile MainNode<K, V> mainnode` reached the Scala subclass as a raw
//!    `MainNode`, and an inherited `K key` as `Object`.
//!
//! 4. **Reading a Java instance field emitted an illegal method name.**
//!    `classpath::fill_java_members` stores a field's *descriptor* in
//!    `jvm_name`, but the backend's term-read path reads `jvm_name` as "the
//!    accessor to call" (which is what it means for a pickled Scala `val`).
//!    So `new jp.Plain("hi").label` emitted
//!    `invokevirtual jp/Plain."Ljava$divlang$divString;":()Ljava/lang/String;`
//!    and the class would not load: `java.lang.ClassFormatError: Illegal
//!    method name`. Entirely pre-existing and not generic-specific -- *any*
//!    Scala read of *any* Java instance field produced an unloadable class
//!    file. The unqualified path additionally had no `static` arm at all, so
//!    a Java static reached through `import C._` emitted `getfield` and died
//!    with `IncompatibleClassChangeError: Expected non-static field`.
//!
//! Measured with `tests/scalalib_measure.sh`: `errors=852
//! files_with_errors=145` before, and TrieMap.scala alone 46 -- the worst
//! file by a wide margin.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with the
//! other slices running in parallel; see `.agent-brief.md`.

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
        "scala-rs-triemapjava-{tag}-{}-{nanos}-{seq}",
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

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn javac_available() -> bool {
    Command::new("javac")
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

fn compile_errors(name: &str, extra: &[&str]) -> String {
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
    let _ = fs::remove_dir_all(&out);
    err
}

fn run_main(out: &Path, main: &str, cp_extra: &[&str]) -> String {
    let mut cp = out.display().to_string();
    for e in cp_extra {
        cp.push(':');
        cp.push_str(e);
    }
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// (1) The self-instantiating constructor type-argument list. Plain Scala; no
// Java anywhere, which is the point.
// ---------------------------------------------------------------------------

/// Private-runtime run (`--no-scala-library`).
#[test]
fn fixtures_tmj_selfctor() {
    let out = compile_fixture_with("tmj_selfctor", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_main(&out, "tmj.Main", &[]),
            expected_stdout("tmj_selfctor")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// library-ABI run (`--scala-library <jar>`).
#[test]
fn fixtures_tmj_selfctor_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("tmj_selfctor", &["--scala-library", jar_s]);
    assert_eq!(
        run_main(&out, "tmj.Main", &[jar_s]),
        expected_stdout("tmj_selfctor")
    );
    let _ = fs::remove_dir_all(&out);
}

/// What believing a written type-argument list must not let through. Real
/// scalac 2.13.16 reports exactly these two, at lines 14 and 19.
#[test]
fn fixtures_tmj_selfctor_bad_is_error() {
    let jar = scala_library_jar();
    let mut modes: Vec<Vec<&str>> = vec![vec!["--no-scala-library"]];
    let jar_s;
    if let Some(j) = &jar {
        jar_s = j.to_str().unwrap();
        modes.push(vec!["--scala-library", jar_s]);
    } else {
        eprintln!("skip scala-library mode: jar not obtainable");
    }
    for extra in modes {
        let err = compile_errors("tmj_selfctor_bad", &extra);
        assert!(
            err.contains("found: Cell[V, K]") && err.contains("required: Cell[K, V]"),
            "expected the swapped-parameter mismatch, got: {err}"
        );
        assert!(
            err.contains("found: K") && err.contains("required: String"),
            "expected the disagreeing written type argument to be rejected, got: {err}"
        );
        assert!(
            err.contains("2 error(s)"),
            "expected exactly 2 errors, as real scalac reports, got: {err}"
        );
    }
}

// ---------------------------------------------------------------------------
// (2)-(4) The Java half: a real `.java`, compiled by javac and read back as a
// class file, which is the only way this compiler sees Java -- and the same
// arrangement `tests/scalalib_measure.sh` uses for the library's own 32 Java
// sources.
// ---------------------------------------------------------------------------

fn compile_jbase() -> PathBuf {
    let src = fixtures_dir().join("java/tmjava/JBase.java");
    let out = tmp_dir("jbase-java");
    let status = Command::new("javac")
        .args(["-d", out.to_str().unwrap(), src.to_str().unwrap()])
        .status()
        .expect("javac");
    assert!(status.success(), "javac tmjava.JBase failed");
    assert!(
        out.join("tmjava/JBase.class").is_file(),
        "tmjava/JBase.class missing"
    );
    out
}

/// A Scala class extending a *generic* Java class that exists only as a class
/// file: it instantiates itself with its own type parameters in a method with
/// no declared result type (1), reads the superclass's package-private statics
/// both qualified and through `import JBase._` (2), reads inherited fields at
/// their generic types (3), and the result has to actually load and run (4).
#[test]
fn fixtures_tmj_java() {
    if !javac_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let java_cp = compile_jbase();
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join("tmj_java.scala");
    let out = tmp_dir("tmj_java");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            java_cp.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("compile tmj_java");
    assert!(
        output.status.success(),
        "compile tmj_java failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    if java_available() {
        let got = run_main(&out, "tmjava.Main", &[java_cp.to_str().unwrap(), jar_s]);
        assert_eq!(got, expected_stdout("tmj_java"));
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&java_cp);
}

/// Admitting default-access members must not make them reachable from another
/// package. Real scalac 2.13.16 reports exactly these two, at lines 12 and 14.
#[test]
fn fixtures_tmj_java_bad_is_error() {
    if !javac_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let java_cp = compile_jbase();
    let src = fixtures_dir().join("tmj_java_bad.scala");
    let out = tmp_dir("tmj_java_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            java_cp.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("compile tmj_java_bad");
    assert!(
        !output.status.success(),
        "expected compile of tmj_java_bad to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for name in ["SENTINEL", "RESTART"] {
        assert!(
            err.contains(&format!("value {name} in class JBase cannot be accessed")),
            "expected {name} to be inaccessible from another package, got: {err}"
        );
    }
    assert!(
        !err.contains("PUBLIC_TAG"),
        "the public static must stay reachable, got: {err}"
    );
    // The generic static: only the field's `Signature` attribute says
    // `JBase[String, Integer]`, and reading the erased descriptor instead
    // gives a raw `JBase` that conforms to every instantiation. This one is
    // a *soundness* case, not an access case -- `PROTOTYPE` is public.
    assert!(
        err.contains("found: JBase[String, Integer]")
            && err.contains("required: JBase[Integer, String]"),
        "expected the generic static field's type argument to be checked, got: {err}"
    );
    assert!(
        err.contains("3 error(s)"),
        "expected exactly 3 errors, as real scalac reports, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&java_cp);
}
