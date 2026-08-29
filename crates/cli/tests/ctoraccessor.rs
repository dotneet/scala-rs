//! E2E tests for the `agent/ctoraccessor` slice:
//!
//! * a constructor parameter is a public accessor, so it implements a parent's
//!   abstract member -- including the `case class` parameters that become
//!   `val`s without the keyword, which type-checked and then died with an
//!   `AbstractMethodError` at run time,
//! * `FunctionN.tupled` / `curried` and `scala.Function.untupled` (arities
//!   2..22), which every `CompiledFunction` in slick's
//!   `lifted/CompilableFunctions.scala` is built out of,
//! * `scala.collection.mutable.Builder`'s `+=` / `++=`, inherited from
//!   `Growable` and returning `this.type`.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new fixtures
//! use the `ctacc` prefix.

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
        "scala-rs-ctacc-{tag}-{}-{nanos}-{seq}",
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

/// `-Xverify:all`, so a missing accessor or a mis-shaped `apply` is a
/// verification failure here rather than a silent difference in the output.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(e) => format!("{}:{}", out.display(), e),
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

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
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
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded expectation,
/// scalac's stdout and ours all have to agree.
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
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );

    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// Constructor accessors are ours, not the library's: the private runtime has
/// to produce exactly the same program.
#[test]
fn fixtures_ctacc_private_runtime() {
    let out = compile_fixture_with("ctacc", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout("ctacc"),
            "stdout mismatch for private-runtime ctacc"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn scala_library_dual_run_ctacc() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run ctacc: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ctacc", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("ctacc"),
        "stdout mismatch for library dual-run ctacc"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_ctacc() {
    real_scalac_dual_run("ctacc");
}

/// The accessor a `case class` parameter turns into, and the bridge it needs
/// when the parent declares the member with a wider (erased) result. Without
/// them `ConstRep` links but throws `AbstractMethodError` on the first call
/// through `Rep`, so the shape is asserted directly rather than only through
/// the program's output.
#[test]
fn ctacc_case_class_params_get_public_accessors() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip ctacc accessor shape: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("ctacc", &["--scala-library", jar.to_str().unwrap()]);
    // `ConstRep[T](value: T)`: one `value()Object`, implementing `Rep.value`.
    assert_eq!(
        method_descs(&out.join("ConstRep.class"), "value"),
        vec!["()Ljava/lang/Object;".to_string()],
    );
    // `NumRep(n: Int)`: a primitive accessor, same erasure as `Named.n`.
    assert_eq!(
        method_descs(&out.join("NumRep.class"), "n"),
        vec!["()I".to_string()],
    );
    // `IntBox(unwrap: Int) extends Boxed { def unwrap: Any }`: accessor plus
    // the bridge to the parent's erased `()Object`.
    assert_eq!(
        method_descs(&out.join("IntBox.class"), "unwrap"),
        vec!["()I".to_string(), "()Ljava/lang/Object;".to_string()],
    );
    assert_eq!(
        method_descs(&out.join("StringBox.class"), "label"),
        vec![
            "()Ljava/lang/String;".to_string(),
            "()Ljava/lang/Object;".to_string(),
        ],
    );
    // Only the first parameter list becomes accessors, as in nsc.
    assert_eq!(
        method_descs(&out.join("Multi.class"), "a"),
        vec!["()I".to_string()],
    );
    assert!(
        method_descs(&out.join("Multi.class"), "extra").is_empty(),
        "a secondary parameter list must not become an accessor"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Every `public` method named `name` in `class_file`, as JVM descriptors, in
/// declaration order. Read with `javap -p`, so the test states the same thing
/// the reference `javap -p -c` output does.
fn method_descs(class_file: &Path, name: &str) -> Vec<String> {
    let out = Command::new("javap")
        .args(["-p", "-s", class_file.to_str().unwrap()])
        .output()
        .expect("javap");
    assert!(
        out.status.success(),
        "javap {} failed: {}",
        class_file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut descs = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let t = line.trim();
        // A method line ends in `);`, a field line does not.
        if !t.ends_with(");") {
            continue;
        }
        let Some(open) = t.find('(') else { continue };
        let head = &t[..open];
        if head.split_whitespace().last() != Some(name) {
            continue;
        }
        for next in lines.by_ref() {
            if let Some(d) = next.trim().strip_prefix("descriptor: ") {
                descs.push(d.to_string());
                break;
            }
        }
    }
    descs
}

/// `f.tupled` / `f.curried` / `Function.untupled`, against the real library.
#[test]
fn scala_library_dual_run_ctacc_fn() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run ctacc_fn: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ctacc_fn", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("ctacc_fn"),
        "stdout mismatch for library dual-run ctacc_fn"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_ctacc_fn() {
    real_scalac_dual_run("ctacc_fn");
}

/// `tupled` / `curried` are `scala/FunctionN` default methods and `untupled`
/// lives on `scala/Function$`; the private runtime emits neither, so
/// `--no-scala-library` must diagnose them rather than emit a call that is not
/// there.
#[test]
fn fixtures_ctacc_fn_without_library_is_error() {
    compile_fails(
        "ctacc_fn",
        &["--no-scala-library"],
        "value tupled is not a member of (Int, Int) => Int",
    );
    compile_fails(
        "ctacc_fn",
        &["--no-scala-library"],
        "value curried is not a member of (Int, Int) => Int",
    );
    compile_fails(
        "ctacc_fn",
        &["--no-scala-library"],
        "not found: value Function",
    );
}

/// `Builder.++=` comes from `Growable` and returns `this.type`.
#[test]
fn scala_library_dual_run_ctacc_builder() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run ctacc_builder: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ctacc_builder", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("ctacc_builder"),
        "stdout mismatch for library dual-run ctacc_builder"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_ctacc_builder() {
    real_scalac_dual_run("ctacc_builder");
}

/// The private runtime has no `scala.collection.mutable` at all.
#[test]
fn fixtures_ctacc_builder_without_library_is_error() {
    compile_fails(
        "ctacc_builder",
        &["--no-scala-library"],
        "not found: type Builder",
    );
}

/// Only a `case class`'s first parameter list becomes accessors on its own: a
/// plain class's parameter without `val` stays private state, and reading it
/// from outside is still an error. (nsc words this "value hidden is not a
/// member of Plain"; we report the access rather than the absence, since the
/// constructor field is a symbol here.)
#[test]
fn fixtures_ctacc_plain_param_bad_is_error() {
    compile_fails(
        "ctacc_plain_bad",
        &["--no-scala-library"],
        "value hidden cannot be accessed as a member of Plain",
    );
}
