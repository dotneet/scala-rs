//! Cross-unit Scala variance ABI checks.
//!
//! A JVM `Signature` records generic bounds but not Scala declaration
//! variance. A separately compiled nsc consumer therefore reads the
//! `ScalaSignature` pickle for `+A`, `-A`, and nested `F[+X]` metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn scala_library() -> PathBuf {
    PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar")
}

#[derive(Clone, Debug)]
struct JavaToolchain {
    home: Option<PathBuf>,
    java: PathBuf,
}

fn tool_succeeds(tool: &Path, java_home: Option<&Path>) -> bool {
    let mut cmd = Command::new(tool);
    cmd.arg("-version");
    match java_home {
        Some(home) => {
            cmd.env("JAVA_HOME", home);
        }
        None => {
            cmd.env_remove("JAVA_HOME");
        }
    }
    cmd.output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn java_toolchain() -> Option<JavaToolchain> {
    if let Some(home) = std::env::var_os("JAVA_HOME").map(PathBuf::from) {
        let java = home.join("bin/java");
        let javac = home.join("bin/javac");
        if tool_succeeds(&java, Some(&home)) && tool_succeeds(&javac, Some(&home)) {
            return Some(JavaToolchain {
                home: Some(home),
                java,
            });
        }
    }

    let java = Path::new("java");
    if tool_succeeds(java, None) && tool_succeeds(Path::new("javac"), None) {
        return Some(JavaToolchain {
            home: None,
            java: java.to_path_buf(),
        });
    }
    None
}

fn scalac(java: &JavaToolchain) -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    let candidates = [cached, PathBuf::from("scalac")];
    candidates.into_iter().find(|candidate| {
        if candidate.is_absolute() && !candidate.is_file() {
            return false;
        }
        let mut cmd = Command::new(candidate);
        cmd.arg("-version");
        with_java_toolchain(&mut cmd, java);
        cmd.output()
            .map(|out| {
                out.status.success() && diagnostics(&out).contains("Scala compiler version 2.13.16")
            })
            .unwrap_or(false)
    })
}

fn tmp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let out = std::env::temp_dir().join(format!("scala-rs-variance-{nanos}"));
    fs::create_dir_all(&out).expect("create variance temp directory");
    out
}

fn with_java_toolchain(cmd: &mut Command, java: &JavaToolchain) {
    if let Some(home) = &java.home {
        let mut paths = vec![home.join("bin")];
        if let Some(old_path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&old_path));
        }
        let path = std::env::join_paths(paths).expect("construct Java PATH");
        cmd.env("JAVA_HOME", home).env("PATH", path);
    } else {
        cmd.env_remove("JAVA_HOME");
    }
}

fn compile_scala_rs(src: &Path, out: &Path, jar: &Path, java: &JavaToolchain) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    with_java_toolchain(&mut cmd, java);
    cmd.output().expect("run scala-rs producer")
}

fn compile_scalac(
    src: &Path,
    out: &Path,
    cp: &Path,
    jar: &Path,
    scalac: &Path,
    java: &JavaToolchain,
) -> Output {
    let classpath = format!("{}:{}", cp.display(), jar.display());
    let mut cmd = Command::new(scalac);
    cmd.args([
        "-Xno-forwarders",
        "-classpath",
        &classpath,
        "-d",
        out.to_str().unwrap(),
        src.to_str().unwrap(),
    ]);
    with_java_toolchain(&mut cmd, java);
    cmd.output().expect("run scalac")
}

fn diagnostics(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn run_java(classes: &Path, producer: &Path, jar: &Path, java: &JavaToolchain) -> String {
    let classpath = format!(
        "{}:{}:{}",
        classes.display(),
        producer.display(),
        jar.display()
    );
    let mut cmd = Command::new(&java.java);
    cmd.args(["-Xverify:all", "-cp", &classpath, "VarianceExternalGood"]);
    with_java_toolchain(&mut cmd, java);
    let out = cmd.output().expect("run variance consumer");
    assert!(out.status.success(), "java failed: {}", diagnostics(&out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn nsc_consumers_preserve_class_method_and_hk_variance() {
    let jar = scala_library();
    let Some(java) = java_toolchain() else {
        eprintln!("skip: Java java/javac is unavailable in JAVA_HOME or PATH");
        return;
    };
    let Some(scalac) = scalac(&java) else {
        eprintln!("skip: Scala compiler 2.13.16 is unavailable in the cache or PATH");
        return;
    };
    if !jar.is_file() {
        eprintln!("skip: Scala library 2.13.16 fixture is unavailable");
        return;
    }
    let root = tmp_dir();
    let producer = fixtures_dir().join("variance_external_producer.scala");
    let good = fixtures_dir().join("variance_external_consumer_good.scala");
    let bad = fixtures_dir().join("variance_external_consumer_bad.scala");
    let nsc_producer = root.join("nsc-producer");
    let ours_producer = root.join("ours-producer");
    let nsc_good = root.join("nsc-good");
    let ours_good = root.join("ours-good");
    let nsc_bad = root.join("nsc-bad");
    let ours_bad = root.join("ours-bad");
    for out in [
        &nsc_producer,
        &ours_producer,
        &nsc_good,
        &ours_good,
        &nsc_bad,
        &ours_bad,
    ] {
        fs::create_dir_all(out).unwrap();
    }

    let out = compile_scalac(&producer, &nsc_producer, &root, &jar, &scalac, &java);
    assert!(
        out.status.success(),
        "nsc producer failed: {}",
        diagnostics(&out)
    );
    let out = compile_scala_rs(&producer, &ours_producer, &jar, &java);
    assert!(
        out.status.success(),
        "scala-rs producer failed: {}",
        diagnostics(&out)
    );

    // nsc is the reference producer/consumer pair, and must accept the same
    // class variance, method type parameter, and higher-kinded variance shape.
    let out = compile_scalac(&good, &nsc_good, &nsc_producer, &jar, &scalac, &java);
    assert!(
        out.status.success(),
        "nsc consumer failed: {}",
        diagnostics(&out)
    );
    assert_eq!(run_java(&nsc_good, &nsc_producer, &jar, &java), "dog:dog\n");

    // The real interop boundary: nsc reads scala-rs's pickle and must accept
    // both legal assignments and the nested `F[+X]` kind.
    let out = compile_scalac(&good, &ours_good, &ours_producer, &jar, &scalac, &java);
    assert!(
        out.status.success(),
        "nsc consumer rejected scala-rs producer: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_java(&ours_good, &ours_producer, &jar, &java),
        "dog:dog\n"
    );

    for (consumer, producer_out) in [(&nsc_bad, &nsc_producer), (&ours_bad, &ours_producer)] {
        let out = compile_scalac(&bad, consumer, producer_out, &jar, &scalac, &java);
        let text = diagnostics(&out);
        assert!(
            !out.status.success(),
            "illegal variance was accepted: {text}"
        );
        let lines: Vec<&str> = text.lines().collect();
        for (source_line, expected) in [
            (2, "VarianceSource[VarianceDog]"),
            (3, "VarianceSink[VarianceAnimal]"),
            (4, "VarianceBox[VarianceDog]"),
        ] {
            let marker =
                format!("variance_external_consumer_bad.scala:{source_line}: error: type mismatch");
            let matches = lines.iter().filter(|line| line.contains(&marker)).count();
            assert_eq!(
                matches, 1,
                "expected one type mismatch for source line {source_line}, found {matches}: {text}"
            );
            let marker_index = lines
                .iter()
                .position(|line| line.contains(&marker))
                .expect("type mismatch marker was counted");
            let diagnostic = lines[marker_index..(marker_index + 4).min(lines.len())].join("\n");
            assert!(
                diagnostic.contains(expected),
                "missing {expected:?} near source line {source_line}: {text}"
            );
        }
    }

    let _ = fs::remove_dir_all(root);
}
