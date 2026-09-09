//! Inherited abstract results guide inference without fixing its final type.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn root() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "absresult-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
fn fixture(n: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(n)
}
fn check(r: &Output) {
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
}
fn compile(files: &[PathBuf], out: &Path, cp: &str, oracle: bool, private: bool) -> Output {
    fs::create_dir_all(out).unwrap();
    let mut c = if oracle {
        Command::new(NSC)
    } else {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
        c.arg("compile");
        if private {
            c.arg("--no-scala-library");
        } else {
            c.args(["--scala-library", JAR]);
        }
        c
    };
    c.args(files)
        .arg("-d")
        .arg(out)
        .args(["-cp", cp])
        .output()
        .unwrap()
}
fn run(out: &Path, cp: &str) -> Output {
    Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            "Main",
        ])
        .output()
        .unwrap()
}
#[test]
fn abstract_results_match_scalac_from_source_and_binary() {
    let p = root();
    let lib = p.join("lib");
    check(&compile(
        &[fixture("absresult_lib.scala")],
        &lib,
        JAR,
        true,
        false,
    ));
    let cp = format!("{}:{JAR}", lib.display());
    let nsc = p.join("nsc");
    check(&compile(
        &[fixture("absresult.scala")],
        &nsc,
        &cp,
        true,
        false,
    ));
    let expected = run(&nsc, &cp);
    check(&expected);
    assert_eq!(
        expected.stdout,
        fs::read(fixture("expected/absresult.txt")).unwrap()
    );
    for binary in [false, true] {
        let files = if binary {
            vec![fixture("absresult.scala")]
        } else {
            vec![fixture("absresult.scala"), fixture("absresult_lib.scala")]
        };
        let path = if binary { &cp } else { JAR };
        let out = p.join(if binary { "binary" } else { "source" });
        check(&compile(&files, &out, path, false, false));
        let actual = run(&out, path);
        check(&actual);
        assert_eq!(actual.stdout, expected.stdout);
    }
    for oracle in [false, true] {
        let bad = compile(
            &[fixture("absresult_bad.scala")],
            &p.join(if oracle { "bad-nsc" } else { "bad-ours" }),
            &cp,
            oracle,
            false,
        );
        assert!(
            !bad.status.success(),
            "accepted incompatible abstract implementation"
        );
        assert!(
            String::from_utf8_lossy(&bad.stderr).contains("type mismatch"),
            "{bad:?}"
        );
    }
    fs::remove_dir_all(p).unwrap();
}
