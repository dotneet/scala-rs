//! Implicit instances chosen by a structural bound, as ScalaTest's
//! `Length`, `Size` and `Emptiness` companions offer them:
//! `lengthOfAnyRefWithParameterlessLengthMethodForInt[T <: AnyRef { def
//! length: Int }]` beside `lengthOfGenSeq[SEQ <: GenSeq[_]]` and
//! `lengthOfJavaList[JLIST <: java.util.List[_]]`. `xs should have length 3`
//! was "ambiguous implicit" over all of them where nsc finds exactly one:
//! the structural bound was not checked at all, `def length: Int` passed for
//! `def length(): Int`, `Seq`'s `length` was not read from its pickle before
//! the candidates were ranked, and a guessed or partially applied solution
//! escaped the candidate's bound.
//!
//! Programs are compiled by scalac and by scala-rs and run; the output has
//! to agree. The shape rules are also checked where scalac rejects them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-implicit-structural-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn coursier_jar(group: &str, artifact: &str, version: &str) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let jar = home
        .join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2")
        .join(group)
        .join(artifact)
        .join(version)
        .join(format!("{artifact}-{version}.jar"));
    jar.is_file().then_some(jar)
}

fn run_java(cp: &str, main: &str) -> String {
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, main])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "{main} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Compile `fixture` with scala-rs (and scalac, when present) against `cp`,
/// run `main` with `run_cp` added, and compare with `expected/<fixture>.txt`.
fn runs_like_scalac(fixture: &str, cp: &str, run_cp: &str, main: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let src = fixtures_dir().join(format!("{fixture}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join(format!("expected/{fixture}.txt"))).unwrap();
    let classpath = |out: &Path| {
        [
            out.display().to_string(),
            cp.to_string(),
            run_cp.to_string(),
        ]
        .into_iter()
        .chain([jar.display().to_string()])
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(":")
    };
    let ours = tmp_dir("ours");
    let mut cmd = Command::new(bin());
    cmd.arg("compile").arg(&src).arg("-d").arg(&ours);
    if !cp.is_empty() {
        cmd.args(["-cp", cp]);
    }
    let o = cmd
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "scala-rs rejected {fixture}: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let actual = run_java(&classpath(&ours), main);
    let _ = fs::remove_dir_all(&ours);
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("scalac");
        let mut cmd = Command::new(scalac);
        cmd.env("JAVA_OPTS", "-Xmx2g -Xss8m").arg("-nowarn");
        if !cp.is_empty() {
            cmd.args(["-cp", cp]);
        }
        let o = cmd
            .arg("-d")
            .arg(&theirs)
            .arg(&src)
            .output()
            .expect("run scalac");
        assert!(o.status.success(), "scalac rejected {fixture}");
        assert_eq!(run_java(&classpath(&theirs), main), expected);
        let _ = fs::remove_dir_all(&theirs);
    }
    assert_eq!(actual, expected);
}

#[test]
fn structural_and_constructor_instances_are_ranked_like_scalac() {
    runs_like_scalac("implicit_structural_bounds", "", "", "Main");
}

/// A parameterless member does not implement `def m(): T`, nor the other
/// way round; nor does one with parameters. scalac rejects exactly these
/// five lines.
#[test]
fn a_structural_def_needs_the_members_own_parameter_lists() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let out = tmp_dir("shape");
    let o = Command::new(bin())
        .arg("compile")
        .arg(fixtures_dir().join("structural_member_shape_bad.scala"))
        .arg("-d")
        .arg(&out)
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    let _ = fs::remove_dir_all(&out);
    let text =
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "{text}");
    let lines: Vec<usize> = text
        .lines()
        .filter_map(|l| l.split("structural_member_shape_bad.scala:").nth(1))
        .filter_map(|rest| rest.split(':').next()?.parse().ok())
        .collect();
    assert_eq!(lines, vec![13, 14, 15, 16, 17], "{text}");
    assert_eq!(text.matches("type mismatch").count(), 5, "{text}");
}

#[test]
fn scalatest_length_size_and_emptiness_matchers_run_like_scalac() {
    let m = "org/scalatest";
    let jars = [
        coursier_jar(m, "scalatest-core_2.13", "3.2.20"),
        coursier_jar(m, "scalatest-funsuite_2.13", "3.2.20"),
        coursier_jar(m, "scalatest-matchers-core_2.13", "3.2.20"),
        coursier_jar(m, "scalatest-shouldmatchers_2.13", "3.2.20"),
        coursier_jar("org/scalactic", "scalactic_2.13", "3.2.20"),
        coursier_jar(m, "scalatest-compatible", "3.2.20"),
    ];
    let xml = coursier_jar("org/scala-lang/modules", "scala-xml_2.13", "2.4.0");
    if jars.iter().any(Option::is_none) || xml.is_none() {
        eprintln!("skip: ScalaTest 3.2.20 or scala-xml 2.4.0 not in the Coursier cache");
        return;
    }
    let cp = jars
        .iter()
        .flatten()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    runs_like_scalac(
        "implicit_structural_scalatest",
        &cp,
        &xml.unwrap().display().to_string(),
        "stt.Main",
    );
}
