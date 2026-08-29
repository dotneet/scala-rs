//! Unreachable-code elimination in the emitter, plus explicit type application
//! reaching the implicit parameter sections. Kept in its own file so it does not
//! collide with `e2e.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
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
    // macOS clocks are microsecond-grained, so two tests starting in the same
    // tick would otherwise share (and delete) one directory.
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-dead-{tag}-{}-{nanos}-{seq}",
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

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
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
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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

fn compile_fails(name: &str, needle: &str) {
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
    assert!(!output.status.success(), "{name} unexpectedly compiled");
    assert!(
        text.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got:\n{text}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `javap -c` output for one class, split into (method signature, instructions).
fn javap_methods(out: &Path, class: &str) -> Vec<(String, Vec<String>)> {
    let output = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(output.status.success(), "javap {class} failed");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut methods: Vec<(String, Vec<String>)> = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "Code:" {
            in_code = true;
            continue;
        }
        if trimmed.ends_with(");") {
            methods.push((trimmed.to_string(), Vec::new()));
            in_code = false;
            continue;
        }
        if trimmed.ends_with(':') {
            // `Exception table:`, `LineNumberTable:`, ... — the code is over.
            in_code = false;
            continue;
        }
        if !in_code {
            continue;
        }
        // Instruction lines look like `   12: invokevirtual #34 // ...`.
        let Some((off, rest)) = trimmed.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        // Skip `tableswitch` case rows, which are `<key>: <target>`.
        if off.parse::<u32>().is_err() || !rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        if let Some(m) = methods.last_mut() {
            m.1.push(rest.to_string());
        }
    }
    methods
}

fn mnemonic(instr: &str) -> &str {
    instr.split_whitespace().next().unwrap_or("")
}

#[test]
fn fixtures_dead() {
    let out = compile_fixture_with("dead", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(run_java(&out, None), expected_stdout("dead"));
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn dead_scala_library_dual_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("dead", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(run_java(&out, Some(&jar)), expected_stdout("dead"));
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn dead_bad_is_error() {
    // Unreachable code is still typechecked.
    compile_fails("dead_bad", "type mismatch");
}

/// `def boom(): Int = throw e` used to emit `athrow; ireturn`, and the trailing
/// `ireturn` popped an empty stack (`VerifyError: Operand stack underflow`).
#[test]
fn dead_no_instruction_after_a_throwing_body() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("dead", &["--no-scala-library"]);
    let methods = javap_methods(&out, "Main$");
    for name in ["public int boom();", "public int both(boolean);"] {
        let (_, code) = methods
            .iter()
            .find(|(sig, _)| sig == name)
            .unwrap_or_else(|| panic!("no {name} in {methods:?}"));
        assert_eq!(
            code.last().map(|i| mnemonic(i)),
            Some("athrow"),
            "{name} must end at the throw, got {code:?}"
        );
        assert!(
            !code.iter().any(|i| mnemonic(i) == "ireturn"),
            "{name} has an unreachable ireturn: {code:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Dropping the dead tail must never leave a method without a terminator
/// (`VerifyError: Control flow falls through code end`).
#[test]
fn dead_every_method_ends_with_a_terminator() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("dead", &["--no-scala-library"]);
    for class in ["Main", "Main$"] {
        for (sig, code) in javap_methods(&out, class) {
            if code.is_empty() {
                continue;
            }
            let last = mnemonic(code.last().unwrap());
            assert!(
                matches!(
                    last,
                    "return"
                        | "ireturn"
                        | "lreturn"
                        | "freturn"
                        | "dreturn"
                        | "areturn"
                        | "athrow"
                        | "goto"
                        | "goto_w"
                        | "ret"
                        | "tableswitch"
                        | "lookupswitch"
                ),
                "{class}.{sig} ends with {last:?}: {code:?}"
            );
        }
    }
    let _ = fs::remove_dir_all(&out);
}

/// Explicit type arguments have to reach the implicit parameter sections, and
/// the type parameters of the enclosing *class* must not be widened away.
/// Library-only: the repeated parameter needs the jar's `Seq` at run time.
#[test]
fn dead_targs_scala_library_dual_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("dead_targs", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(run_java(&out, Some(&jar)), expected_stdout("dead_targs"));
    let _ = fs::remove_dir_all(&out);
}

/// Narrowing an overload by the explicit type argument must not silently fill
/// an implicit that has no witness.
#[test]
fn dead_targs_bad_is_error() {
    compile_fails(
        "dead_targs_bad",
        "could not find implicit value of type TT[String]",
    );
}
