//! Two unrelated gaps, each verified against real scalac 2.13.16.
//!
//! 1. **`Unit` as a comparison operand** (`ue_eq*`). `agent/unitbox` put
//!    `scala/runtime/BoxedUnit` in every `Unit` *value* position -- parameters,
//!    fields, array elements, type arguments -- but the operands of `==` /
//!    `!=` and the *receiver* of a member selected on a `Unit` were left out.
//!    A `Unit` expression leaves nothing on the operand stack, so `() == ()`
//!    popped what was never pushed:
//!
//!    ```text
//!    java.lang.VerifyError: Operand stack underflow
//!      Location: Main$.main([Ljava/lang/String;)V @3: invokestatic
//!    ```
//!
//!    and the class only failed at run time -- the compile was silent. Same for
//!    `().toString`, `().hashCode`, `().isInstanceOf[Unit]`, `().getClass`.
//!    scalac warns (`comparing values of types Unit and Unit using == will
//!    always yield true`) and emits `true`.
//!
//! 2. **`scala.Enumeration`** (`ue_enum*`). `object Color extends Enumeration`
//!    with `val Red, Green, Blue = Value`, the four `Value` overloads,
//!    `values`, `withName`, `Value.id`, and `case Color.Red =>`.
//!
//! Every fixture that can run in both modes is run three ways -- private
//! runtime, real `scala-library` jar, and real scalac -- and all three have to
//! print the same thing. `-Xverify:all` so a bad descriptor or a wrong
//! `StackMapTable` fails rather than passing by luck.

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
        "scala-rs-uniteq-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn compile(out: &Path, name: &str, extra: &[&str]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", out.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn jar_run(name: &str) {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip {name}: jar or java not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    assert_eq!(
        run_main(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for {name} (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

fn private_run(name: &str) {
    if !java_available() {
        return;
    }
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--no-scala-library"]);
    assert!(ok, "compile {name} --no-scala-library failed:\n{msgs}");
    assert_eq!(
        run_main(&out, None),
        expected_stdout(name),
        "stdout mismatch for {name} (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The recorded expectation has to be what real scalac 2.13.16 prints.
fn matches_real_scalac(name: &str) {
    let (Some(scalac), Some(jar), true) = (find_scalac(), scala_library_jar(), java_available())
    else {
        eprintln!("skip real-scalac diff {name}: scalac, jar or java not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-nsc"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    assert_eq!(
        run_main(&ref_out, Some(&jar)),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// A fixture the private runtime cannot support must say so, not be quietly
/// mis-compiled.
fn private_compile_fails(name: &str, needles: &[&str]) {
    let out = tmp_dir(&format!("{name}-priv"));
    let (ok, msgs) = compile(&out, name, &["--no-scala-library"]);
    assert!(
        !ok,
        "expected --no-scala-library compile of {name} to fail, got:\n{msgs}"
    );
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in --no-scala-library diagnostics for {name}, got {msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: jar not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn disassemble(class_file: &Path) -> String {
    let out = Command::new("javap")
        .args(["-p", "-c", class_file.to_str().unwrap()])
        .output()
        .expect("javap");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// -------------------------------------------------- 1. `Unit` as an operand

#[test]
fn ue_eq_private_runtime() {
    private_run("ue_eq");
}

#[test]
fn ue_eq_scala_library() {
    jar_run("ue_eq");
}

#[test]
fn ue_eq_matches_real_scalac() {
    matches_real_scalac("ue_eq");
}

#[test]
fn ue_eqlib_scala_library() {
    jar_run("ue_eqlib");
}

#[test]
fn ue_eqlib_matches_real_scalac() {
    matches_real_scalac("ue_eqlib");
}

/// `##` needs `scala.runtime.Statics` and the rest needs varargs `List.apply`
/// / `Set` / `Map` / `Function2`; the private runtime has none of them and
/// must say so rather than emit something that does not load.
#[test]
fn ue_eqlib_private_runtime_is_diagnosed() {
    private_compile_fails("ue_eqlib", &["error"]);
}

#[test]
fn ue_eq_bad_is_rejected() {
    compile_fails(
        "ue_eq_bad",
        &[
            "type mismatch",
            "value eq is not a member",
            "value length is not a member",
        ],
    );
}

/// The bytecode itself: both operands of `() == ()` are pushed. A run alone
/// cannot prove this -- the old output *compiled*, and only the verifier
/// noticed.
#[test]
fn ue_eq_pushes_both_operands() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: jar not present");
        return;
    };
    let out = tmp_dir("ue_eq-desc");
    let (ok, msgs) = compile(&out, "ue_eq", &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "compile ue_eq failed:\n{msgs}");
    let code = disassemble(&out.join("Main$.class"));
    let main: Vec<&str> = code
        .lines()
        .skip_while(|l| !l.contains("public void main("))
        .collect();
    let idx = main
        .iter()
        .position(|l| l.contains("BoxesRunTime.equals"))
        .expect("no BoxesRunTime.equals in main");
    // The two instructions before the first comparison are the two operands.
    let unit = "BoxedUnit.UNIT";
    assert!(
        main[idx - 1].contains(unit) && main[idx - 2].contains(unit),
        "`() == ()` does not push two operands:\n{}",
        main[..=idx].join("\n")
    );
    let _ = fs::remove_dir_all(&out);
}

// --------------------------------------------------- 2. `scala.Enumeration`

#[test]
fn ue_enum_scala_library() {
    jar_run("ue_enum");
}

#[test]
fn ue_enum_matches_real_scalac() {
    matches_real_scalac("ue_enum");
}

/// `scala.Enumeration` is read out of the real jar; the private runtime has no
/// such class, so `--no-scala-library` has to diagnose it instead of inventing
/// one.
#[test]
fn ue_enum_private_runtime_is_diagnosed() {
    private_compile_fails("ue_enum", &["error", "not found: value Value"]);
}

#[test]
fn ue_enum_bad_is_rejected() {
    compile_fails(
        "ue_enum_bad",
        &[
            "no matching overload",
            "value nosuchMember is not a member",
            "type mismatch",
        ],
    );
}
