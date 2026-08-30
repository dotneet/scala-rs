//! A *call* whose static return type is `Nothing` (`sys.error(...)`, `???`, a
//! user `def die(): Nothing = ...`) used to leave a real `scala/runtime/
//! Nothing$` reference on the JVM operand stack with nothing to consume it,
//! wherever it sat in a `match`/`if`/`try` arm, a block tail, a whole method
//! body, or an argument position. The type checker accepted it (`Nothing`
//! conforms to everything), but classloading rejected the bytecode:
//! `VerifyError: Inconsistent stackmap frames at branch target N` when the
//! arm joined with a differently-typed sibling (`Tuple2`, `Int`, ...), or
//! `VerifyError: Operand stack underflow` / `Method expects a return value`
//! once the descriptor fix below was in and a return path needed the right
//! opcode. `nsc` avoids all of this by following such a call with `athrow`
//! (confirmed with `javap -c` on real scalac output — see `gen_expr` in
//! `crates/backend/src/gen.rs`), making everything after it dead code; this
//! backend now does the same. Kept in its own file so it does not collide
//! with `e2e.rs`.

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
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-nc-{tag}-{}-{nanos}-{seq}",
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

/// Runs `java -Xverify:all -cp <out>[:<jar>] Main`. Verification is the whole
/// point of this suite, so it is never skipped when `java` is available.
fn run_java_verified(out: &Path, cp_extra: Option<&Path>) -> String {
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
        "java -Xverify:all Main failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
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
            in_code = false;
            continue;
        }
        if !in_code {
            continue;
        }
        let Some((off, rest)) = trimmed.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
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

// ---------------------------------------------------------------------------
// `nc_nothing`: match/if/try arms, a block tail, a whole method body, an
// argument position and an ascription, all ending in a `Nothing`-typed call
// (a user `die(): Nothing`, or `???`) — private-runtime safe.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_nc_nothing() {
    let out = compile_fixture_with("nc_nothing", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(run_java_verified(&out, None), expected_stdout("nc_nothing"));
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn nc_nothing_scala_library_dual_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("nc_nothing", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java_verified(&out, Some(&jar)),
        expected_stdout("nc_nothing")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The original repro: a `sys.error(...)` (not an explicit `throw`) as a
/// `match` arm joining with a `Tuple2`-producing sibling, an `if` arm, and
/// inside a by-name argument (`Option.getOrElse`). Needs the real jar for
/// `sys` and `Option`.
#[test]
fn nc_nothing_sys_scala_library_dual_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(
        "nc_nothing_sys",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_java_verified(&out, Some(&jar)),
        expected_stdout("nc_nothing_sys")
    );
    let _ = fs::remove_dir_all(&out);
}

/// A method whose *whole* body is a `Nothing`-typed call (no live sibling
/// path) has to end its bytecode right at the `athrow` that call grew —
/// nothing trailing (a stray `goto`, a `pop`, a value-producing op, a
/// `return`) may remain live, or a still-emitted-but-dead successor
/// desyncs the descriptor's promised return from what the code actually
/// does. Matches real scalac's `die()`/`g()` shape exactly (no `ireturn`
/// after the `athrow` at all).
#[test]
fn nc_nothing_wholly_diverging_methods_end_at_athrow() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("nc_nothing", &["--no-scala-library"]);
    let methods = javap_methods(&out, "Main$");
    for name in [
        "public scala.runtime.Nothing$ die();",
        "public int blockTail();",
        "public int wholeBody();",
        "public int ascribed();",
        "public void argPosition();",
    ] {
        let (_, code) = methods
            .iter()
            .find(|(sig, _)| sig == name)
            .unwrap_or_else(|| panic!("no {name} in {methods:?}"));
        assert_eq!(
            code.last().map(|i| mnemonic(i)),
            Some("athrow"),
            "{name} must end at the throw, got {code:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// A `match`/`if`/`try` arm that ends in a `Nothing`-typed call still has to
/// grow its own `athrow` (this is what used to be missing — the bare
/// `invoke` left a real reference flowing into the join with the live
/// sibling arm, which is exactly the reported `VerifyError`). The method as
/// a whole ends at the *live* arm's `return`, since that is the reachable
/// join, not at the dead arm's `athrow`.
#[test]
fn nc_nothing_diverging_arms_still_grow_an_athrow() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("nc_nothing", &["--no-scala-library"]);
    let methods = javap_methods(&out, "Main$");
    for name in [
        "public scala.Tuple2 matchArm(int);",
        "public int ifArm(int);",
        "public int tryArm(int);",
    ] {
        let (_, code) = methods
            .iter()
            .find(|(sig, _)| sig == name)
            .unwrap_or_else(|| panic!("no {name} in {methods:?}"));
        assert!(
            code.iter().any(|i| mnemonic(i) == "athrow"),
            "{name} must throw on its diverging arm, got {code:?}"
        );
        let last = code.last().map(|i| mnemonic(i));
        assert!(
            matches!(last, Some("areturn") | Some("ireturn")),
            "{name} must still end at the live arm's return, got {code:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// A user-defined `def die(): Nothing` erases to `()Lscala/runtime/Nothing$;`
/// (never `()V`, unlike `Unit`) — matches `javap -c` on real scalac's
/// `T1.die()`. Calling `die()` in tail position of another `Nothing`-typed
/// method has to hand that reference back with `areturn`, or the call site
/// would invoke a descriptor that promises a value and get none.
#[test]
fn nc_nothing_user_method_descriptor_is_not_void() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("nc_nothing", &["--no-scala-library"]);
    let output = Command::new("javap")
        .args(["-p", "-cp", out.to_str().unwrap(), "Main$"])
        .output()
        .expect("javap");
    assert!(output.status.success(), "javap Main$ failed");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("scala.runtime.Nothing$ die();"),
        "die() must be declared to return Nothing$, not V; got:\n{text}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Dropping the dead tail must never leave a method without a terminator.
#[test]
fn nc_nothing_every_method_ends_with_a_terminator() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("nc_nothing", &["--no-scala-library"]);
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
