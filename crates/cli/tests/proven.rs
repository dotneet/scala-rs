//! Binary conversion witnesses are completed before applicability checks.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn root() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "proven-{}-{}-{}",
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
fn binary_conversion_witnesses_match_scalac() {
    let p = root();
    let lib = p.join("lib");
    check(&compile(
        &[fixture("proven_lib.scala")],
        &lib,
        JAR,
        true,
        false,
    ));
    let cp = format!("{}:{JAR}", lib.display());
    let source = fixture("proven.scala");
    let nsc = p.join("nsc");
    check(&compile(&[source.clone()], &nsc, &cp, true, false));
    let expected = run(&nsc, &cp);
    check(&expected);
    assert_eq!(
        expected.stdout,
        fs::read(fixture("expected/proven.txt")).unwrap()
    );
    let ours = p.join("ours");
    check(&compile(&[source], &ours, &cp, false, false));
    let actual = run(&ours, &cp);
    check(&actual);
    assert_eq!(actual.stdout, expected.stdout);
    for oracle in [false, true] {
        let bad = compile(
            &[fixture("proven_bad.scala")],
            &p.join(if oracle { "bad-nsc" } else { "bad-ours" }),
            &cp,
            oracle,
            false,
        );
        assert!(!bad.status.success(), "accepted incompatible conversion");
        assert!(
            String::from_utf8_lossy(&bad.stderr).contains("type mismatch"),
            "{bad:?}"
        );
    }
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn higher_kinded_results_use_singleton_underlying_types() {
    let p = root();
    let source = fixture("proven_singleton.scala");
    let nsc = p.join("nsc");
    check(&compile(&[source.clone()], &nsc, JAR, true, false));
    let expected = run(&nsc, JAR);
    check(&expected);
    assert_eq!(
        expected.stdout,
        fs::read(fixture("expected/proven_singleton.txt")).unwrap()
    );
    let out = p.join("ours");
    check(&compile(&[source], &out, JAR, false, false));
    let actual = run(&out, JAR);
    check(&actual);
    assert_eq!(actual.stdout, expected.stdout);
    for oracle in [false, true] {
        let bad = compile(
            &[fixture("proven_singleton_bad.scala")],
            &p.join(if oracle { "bad-nsc" } else { "bad-ours" }),
            JAR,
            oracle,
            false,
        );
        assert!(
            !bad.status.success(),
            "accepted singleton narrowing without a conversion"
        );
        assert!(
            String::from_utf8_lossy(&bad.stderr).contains("type mismatch"),
            "{bad:?}"
        );
    }
    fs::remove_dir_all(p).unwrap();
}
