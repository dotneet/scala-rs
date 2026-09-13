//! Regression test for a pickled result prefix that names a concrete receiver.
//!
//! `BasicProfile.API.Database` is declared as `backend.DatabaseFactory`. The
//! JVM descriptor only says `Object`, so the Scala pickle carries the stable
//! path `BasicProfile.this.backend` beside the result type. A second
//! scala-rs compilation must read that path through `Driver.backend`, reduce
//! it to `Factory`, and select `Factory.forURL` from an unqualified import.
//!
//! The two stages deliberately use scala-rs as both writer and reader. The
//! fixture does not require the private runtime: the test uses the cached
//! scala-library jar for both stages and for the JVM run.

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

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|output| output.status.success() || !output.stderr.is_empty())
        .unwrap_or(false)
}

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-resultprefix-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn diagnostics(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn compile(source: &Path, out: &Path, cp: Option<&str>, library: &Path) -> Output {
    let mut command = Command::new(bin());
    command.args([
        "compile",
        source.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    if let Some(cp) = cp {
        command.args(["-cp", cp]);
    }
    command.args(["--scala-library", library.to_str().unwrap()]);
    command.output().expect("run scala-rs compile")
}

#[test]
fn scala_rs_pickle_result_prefix_round_trips_to_factory_member() {
    if !tool_available("java") || !tool_available("jar") {
        eprintln!("skip result prefix round trip: java or jar unavailable");
        return;
    }
    let Some(library) = scala_library_jar() else {
        eprintln!("skip result prefix round trip: scala-library jar unavailable");
        return;
    };

    let root = temp_dir();
    let writer = root.join("writer");
    let reader = root.join("reader");
    fs::create_dir_all(&writer).unwrap();
    fs::create_dir_all(&reader).unwrap();

    let lib = compile(
        &fixtures_dir().join("rpf_lib.scala"),
        &writer,
        None,
        &library,
    );
    assert!(
        lib.status.success(),
        "scala-rs writer failed:\n{}",
        diagnostics(&lib)
    );

    let jar = root.join("resultprefix-lib.jar");
    let packed = Command::new("jar")
        .args([
            "cf",
            jar.to_str().unwrap(),
            "-C",
            writer.to_str().unwrap(),
            "resultprefix",
        ])
        .output()
        .expect("pack scala-rs writer output");
    assert!(
        packed.status.success(),
        "jar failed:\n{}",
        diagnostics(&packed)
    );

    let cp = format!("{}:{}", jar.display(), library.display());
    let use_stage = compile(
        &fixtures_dir().join("rpf_use.scala"),
        &reader,
        Some(&cp),
        &library,
    );
    assert!(
        use_stage.status.success(),
        "scala-rs reader failed:\n{}",
        diagnostics(&use_stage)
    );

    let runtime_cp = format!("{}:{}", reader.display(), cp);
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &runtime_cp, "resultprefix.Client"])
        .output()
        .expect("run result prefix probe");
    assert!(
        run.status.success(),
        "result prefix probe failed:\n{}",
        diagnostics(&run)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "1\n",
        "unexpected Factory.forURL result"
    );

    let _ = fs::remove_dir_all(root);
}
