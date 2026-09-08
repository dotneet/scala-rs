//! The right operand of `scala.Boolean.&&` / `||` is a tail position.
//!
//! `tests/fixtures/trc_bool.scala` reproduces the seven `@tailrec` shapes the
//! 2.13.16 library writes this way, runs each deep case two million times
//! under a 256k stack, and is compared against scalac 2.13.16 compiling the
//! same source. Without the transform every deep case overflows, so a passing
//! run is what proves the calls became backward branches; `javap -c` then
//! confirms no self-`invoke` is left in any of them.
use std::{fs, path::PathBuf, process::Command};

fn prerequisites() -> bool {
    let ready = [
        "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        "/tmp/scala-2.13.16/bin/scalac",
    ]
    .iter()
    .all(|p| PathBuf::from(p).is_file());
    if !ready {
        eprintln!("skip trc_bool differential tests: Scala 2.13.16 compiler and library required");
    }
    ready
}

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("scala-rs-trcbool-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

// Zulu 15.0.6's C2 miscompiles these loops even when scalac emits them (see
// docs/tailrec.md). Prefer Temurin 17; otherwise stay in the interpreter so
// the differential test measures the two Scala compilers and not the JIT.
fn java() -> Command {
    let jdk17 = "/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home/bin/java";
    if PathBuf::from(jdk17).is_file() {
        Command::new(jdk17)
    } else {
        let mut c = Command::new("java");
        c.arg("-Xint");
        c
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

#[test]
fn trc_bool_shortcircuit_dual_run_and_bytecode() {
    if !prerequisites() {
        return;
    }
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let ours = dir("ours");
    let reference = dir("reference");
    let src = fixture("trc_bool.scala");

    let output = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "--scala-library",
            jar,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new("/tmp/scala-2.13.16/bin/scalac")
        .arg(&src)
        .arg("-d")
        .arg(&reference)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = fs::read(fixture("expected/trc_bool.txt")).unwrap();
    for out in [&ours, &reference] {
        let result = java()
            .args([
                "-Xverify:all",
                "-Xss256k",
                "-cp",
                &format!("{}:{jar}", out.display()),
                "TrcBool",
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}: {}",
            out.display(),
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, expected, "{}", out.display());
    }

    for (class, methods) in [
        (
            "TrcBool$",
            vec![
                "allEven", "reaches", "scan", "sameTail", "andShort", "orShort", "wideOr",
            ],
        ),
        ("TrcPing", vec!["bounce"]),
    ] {
        let result = Command::new("javap")
            .args(["-p", "-c", "-classpath", ours.to_str().unwrap(), class])
            .output()
            .unwrap();
        assert!(result.status.success());
        let dis = String::from_utf8(result.stdout).unwrap();
        for name in methods {
            let body = dis
                .split("\n\n")
                .find(|part| {
                    part.lines()
                        .next()
                        .is_some_and(|line| line.contains(&format!(" {name}(")))
                })
                .unwrap_or_else(|| panic!("no body for {name} in {class}"));
            assert!(
                !body.lines().any(
                    |line| line.contains("invoke") && line.contains(&format!("Method {name}:"))
                ),
                "recursive call remains for {name}:\n{body}"
            );
            assert!(
                body.contains("goto"),
                "loop branch missing for {name}:\n{body}"
            );
        }
    }
}

#[test]
fn trc_bool_rejects_what_nsc_rejects() {
    if !prerequisites() {
        return;
    }
    let out = dir("bad");
    // Each of these must stay rejected: the *left* operand of a short circuit,
    // an operand consumed by `!`, a short circuit that is not itself in tail
    // position, a user-defined `||` (strict, so its argument is an ordinary
    // argument), a call in a `val`'s right-hand side, and an overridable
    // method whose recursion is reached through `||`.
    for executable in [
        env!("CARGO_BIN_EXE_scala-rs"),
        "/tmp/scala-2.13.16/bin/scalac",
    ] {
        let mut cmd = Command::new(executable);
        if executable == env!("CARGO_BIN_EXE_scala-rs") {
            cmd.arg("compile").args([
                "--scala-library",
                "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
            ]);
        }
        let result = cmd
            .arg(fixture("trc_bool_bad.scala"))
            .arg("-d")
            .arg(&out)
            .output()
            .unwrap();
        assert!(!result.status.success(), "{executable} accepted the file");
        let diag = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            diag.matches("not in tail position").count(),
            5,
            "{executable}: {diag}"
        );
        assert!(diag.contains("overridden"), "{executable}: {diag}");
    }
}
