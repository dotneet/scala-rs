//! CLI-level ScalaSignature interoperability tests for type-parameter bounds.
//!
//! Every matrix uses real scalac 2.13.16 as the consumer. The nsc-produced
//! provider is the oracle; the scala-rs-produced provider must have the same
//! positive and negative consumer behavior. In particular, the negative
//! `TableQuery[String]` case used to compile when scala-rs dropped the
//! provider's `E <: AbstractTable[_]` bound and therefore guards against that
//! regression at the classpath boundary rather than only inspecting tags.

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
    let dir = std::env::temp_dir().join(format!(
        "scala-rs-existential-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() && is_scala_21316(&cached) {
        return Some(cached);
    }
    let path = PathBuf::from("scalac");
    is_scala_21316(&path).then_some(path)
}

fn is_scala_21316(scalac: &Path) -> bool {
    let output = match Command::new(scalac).arg("-version").output() {
        Ok(output) => output,
        Err(_) => return false,
    };
    let version = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.status.success() && version.contains("2.13.16")
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn source(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn cp(paths: &[&Path]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

fn run_scalac(
    scalac: &Path,
    jar: &Path,
    src: &Path,
    out: &Path,
    provider: Option<&Path>,
) -> (bool, String) {
    let mut cmd = Command::new(scalac);
    cmd.arg("-d").arg(out);
    let classpath = provider
        .map(|p| cp(&[p, jar]))
        .unwrap_or_else(|| jar.display().to_string());
    cmd.arg("-cp").arg(classpath).arg(src);
    let output = cmd.output().expect("run scalac");
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ),
    )
}

fn run_scala_rs(jar: &Path, src: &Path, out: &Path) -> (bool, String) {
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs");
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        ),
    )
}

fn run_main(classes: &Path, provider: &Path, jar: &Path) -> String {
    let classpath = cp(&[classes, provider, jar]);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &classpath, "Main"])
        .output()
        .expect("run java Main");
    assert!(
        output.status.success(),
        "java Main failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_provider_matrix(
    label: &str,
    provider_fixture: &str,
    positive_fixture: &str,
    negative_fixture: &str,
    negative_patterns: &[&[&str]],
    expected_stdout: &str,
) {
    let Some(scalac) = scalac() else {
        eprintln!("skip {label}: scalac 2.13.16 not obtainable");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {label}: scala-library-2.13.16.jar not obtainable");
        return;
    };

    let nsc_provider = tmp_dir(&format!("{label}-nsc-provider"));
    let (ok, msgs) = run_scalac(
        &scalac,
        &jar,
        &source(provider_fixture),
        &nsc_provider,
        None,
    );
    assert!(ok, "nsc provider failed for {label}:\n{msgs}");

    let ours_provider = tmp_dir(&format!("{label}-ours-provider"));
    let (ok, msgs) = run_scala_rs(&jar, &source(provider_fixture), &ours_provider);
    assert!(ok, "scala-rs provider failed for {label}:\n{msgs}");

    for (provider_name, provider) in [("nsc", &nsc_provider), ("scala-rs", &ours_provider)] {
        let positive_out = tmp_dir(&format!("{label}-{provider_name}-positive"));
        let (ok, msgs) = run_scalac(
            &scalac,
            &jar,
            &source(positive_fixture),
            &positive_out,
            Some(provider),
        );
        assert!(
            ok,
            "{provider_name} provider rejected positive {label} consumer:\n{msgs}"
        );
        assert_eq!(
            run_main(&positive_out, provider, &jar),
            expected_stdout,
            "stdout mismatch for {provider_name} provider in {label}"
        );
        let _ = fs::remove_dir_all(&positive_out);

        let negative_out = tmp_dir(&format!("{label}-{provider_name}-negative"));
        let (ok, msgs) = run_scalac(
            &scalac,
            &jar,
            &source(negative_fixture),
            &negative_out,
            Some(provider),
        );
        assert!(
            !ok,
            "{provider_name} provider accepted negative {label} consumer:\n{msgs}"
        );
        for pattern in negative_patterns {
            let found = msgs
                .lines()
                .any(|line| pattern.iter().all(|needle| line.contains(needle)));
            assert!(
                found,
                "{provider_name} provider negative {label} consumer missed diagnostic containing {pattern:?}:\n{msgs}"
            );
        }
        let _ = fs::remove_dir_all(&negative_out);
    }

    let _ = fs::remove_dir_all(nsc_provider);
    let _ = fs::remove_dir_all(ours_provider);
}

#[test]
fn xmeta_bound_provider_matches_nsc_consumer_matrix() {
    compile_provider_matrix(
        "table-query-bound",
        "xmeta_existential_provider",
        "xmeta_bound_positive_consumer",
        "xmeta_bound_negative_consumer",
        &[&[
            "type arguments [String]",
            "class TableQuery's type parameter bounds",
            "AbstractTable[_]",
        ]],
        "concrete\n",
    );
}

#[test]
fn xmeta_bound_shapes_match_nsc_consumer_matrix() {
    compile_provider_matrix(
        "bound-shapes",
        "xmeta_bound_shapes_provider",
        "xmeta_bound_shapes_positive_consumer",
        "xmeta_bound_shapes_negative_consumer",
        &[
            &[
                "type arguments [Any,String]",
                "class Pair's type parameter bounds",
                "[A <: B,B]",
            ],
            &[
                "type arguments [Int]",
                "trait FBound's type parameter bounds",
                "[A <: Comparable[A]]",
            ],
            &[
                "type arguments [Nothing]",
                "trait LowerBound's type parameter bounds",
                "[A >: String]",
            ],
        ],
        "bound-shapes\n",
    );
}
