use std::{fs, path::PathBuf, process::Command};
fn prerequisites() -> bool {
    let ready = [
        "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        "/tmp/scala-2.13.16/bin/scalac",
    ]
    .iter()
    .all(|p| PathBuf::from(p).is_file());
    if !ready {
        eprintln!("skip trc differential tests: Scala 2.13.16 compiler and library required");
    }
    ready
}
fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("scala-rs-trc-{tag}-{}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}
// Zulu 15.0.6 C2 on arm64 miscompiles this switch loop even when scalac
// emits it. Prefer the available Java 17; keep the portable fallback in
// interpreter mode so the differential test measures the Scala compilers.
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
fn trc_deep_dual_run_and_bytecode() {
    if !prerequisites() {
        return;
    }
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    assert!(
        PathBuf::from(jar).exists(),
        "requires Scala 2.13.16 library"
    );
    let ours = dir("ours");
    let reference = dir("reference");
    let src = fixture("trc_deep.scala");
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
    let expected = fs::read(fixture("expected/trc_deep.txt")).unwrap();
    for out in [&ours, &reference] {
        let result = java()
            .args([
                "-Xverify:all",
                "-Xss256k",
                "-cp",
                &format!("{}:{jar}", out.display()),
                "TrcDeep",
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
    let client = dir("client");
    let result = Command::new("/tmp/scala-2.13.16/bin/scalac")
        .arg(fixture("trc_client.scala"))
        .arg("-classpath")
        .arg(&ours)
        .arg("-d")
        .arg(&client)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = java()
        .args([
            "-Xverify:all",
            "-Xss256k",
            "-cp",
            &format!("{}:{}:{jar}", client.display(), ours.display()),
            "TrcClient",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "2000001000000\n2000011\n"
    );
    for class in ["TrcDeep$", "TrcCounter"] {
        let result = Command::new("javap")
            .args(["-p", "-c", "-classpath", ours.to_str().unwrap(), class])
            .output()
            .unwrap();
        assert!(result.status.success());
        let dis = String::from_utf8(result.stdout).unwrap();
        fs::write(ours.join(format!("{class}.javap")), &dis).unwrap();
        for name in if class == "TrcDeep$" {
            vec!["wide", "matching", "order"]
        } else {
            vec!["hop", "count"]
        } {
            let body = dis
                .split("\n\n")
                .find(|part| {
                    part.lines()
                        .next()
                        .is_some_and(|line| line.contains(&format!(" {name}(")))
                })
                .unwrap();
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
fn trc_rejects_non_tail_and_overridable() {
    if !prerequisites() {
        return;
    }
    let out = dir("bad");
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
            .arg(fixture("trc_bad.scala"))
            .arg("-d")
            .arg(&out)
            .output()
            .unwrap();
        assert!(!result.status.success());
        let diag = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(diag.contains("tail position"), "{diag}");
        assert!(diag.contains("overridden"), "{diag}");
    }
}
#[test]
fn trc_valueclass_dual_run_and_static_abi() {
    if !prerequisites() {
        return;
    }
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let src = fixture("trc_valueclass.scala");
    let ours = dir("valueclass-ours");
    let reference_out = dir("valueclass-reference");
    let output = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            src.to_str().unwrap(),
            "--scala-library",
            jar,
            "-d",
            ours.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scala-rs value-class tailrec failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reference = Command::new("/tmp/scala-2.13.16/bin/scalac")
        .arg(&src)
        .arg("-d")
        .arg(&reference_out)
        .output()
        .unwrap();
    assert!(
        reference.status.success(),
        "legal Scala: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let expected = fs::read(fixture("expected/trc_valueclass.txt")).unwrap();
    for out in [&ours, &reference_out] {
        let result = java()
            .args([
                "-Xverify:all",
                "-Xss256k",
                "-cp",
                &format!("{}:{jar}", out.display()),
                "TrcValueclass",
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

    // A separately compiled scalac client must resolve the static extension
    // ABI emitted by scala-rs and run the same loop without boxing the receiver.
    let client = dir("valueclass-client");
    let result = Command::new("/tmp/scala-2.13.16/bin/scalac")
        .arg(fixture("trc_valueclass_client.scala"))
        .arg("-classpath")
        .arg(format!("{}:{jar}", ours.display()))
        .arg("-d")
        .arg(&client)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "scalac value-class client failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = java()
        .args([
            "-Xverify:all",
            "-Xss256k",
            "-cp",
            &format!("{}:{}:{jar}", client.display(), ours.display()),
            "TrcValueclassClient",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "value-class client failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, expected);

    let javap = Command::new("javap")
        .args([
            "-p",
            "-c",
            "-s",
            "-classpath",
            ours.to_str().unwrap(),
            "TrcLong",
        ])
        .output()
        .unwrap();
    assert!(javap.status.success());
    let dis = String::from_utf8(javap.stdout).unwrap();
    let body = dis
        .split("\n\n")
        .find(|part| part.contains("loop$extension"))
        .expect("TrcLong.loop$extension javap body");
    assert!(body.contains("descriptor: (JIJ)J"), "{body}");
    assert!(body.contains("goto"), "tail loop branch missing:\n{body}");
    assert!(
        !body
            .lines()
            .any(|line| line.contains("invoke") && line.contains("loop")),
        "recursive extension call remains:\n{body}"
    );
}
#[test]
fn trc_recursion_in_receiver_and_earlier_argument_is_not_tail() {
    if !prerequisites() {
        return;
    }
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
            .arg(fixture("trc_inputs_bad.scala"))
            .arg("-d")
            .arg(dir("inputs-bad"))
            .output()
            .unwrap();
        assert!(!result.status.success());
        let diag = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            diag.matches("recursive call not in tail position").count() >= 2,
            "{diag}"
        );
    }
}
