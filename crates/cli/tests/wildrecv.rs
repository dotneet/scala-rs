//! Object wildcard imports must preserve the receiver at run time.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const SCALAC: &str = "/tmp/scala-2.13.16/bin/scalac";

fn temp() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "wildrecv-{}-{}-{}",
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
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}
fn compile(src: &Path, out: &Path, oracle: bool, private: bool) -> Output {
    fs::create_dir_all(out).unwrap();
    let mut c = if oracle {
        Command::new(SCALAC)
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
    c.arg(src).arg("-d").arg(out).output().unwrap()
}
fn success(o: &Output) {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
fn run(out: &Path, name: &str, private: bool) -> Output {
    let cp = if private {
        out.display().to_string()
    } else {
        format!("{}:{JAR}", out.display())
    };
    Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, name])
        .output()
        .unwrap()
}
#[test]
fn wildcard_receiver_runs_like_scalac() {
    if !Path::new(SCALAC).exists() || !Path::new(JAR).exists() {
        eprintln!("SKIP: real Scala 2.13.16 is unavailable");
        return;
    }
    let p = temp();
    let src = fixture("wildrecv.scala");
    let nsc = p.join("nsc");
    success(&compile(&src, &nsc, true, false));
    let expected = run(&nsc, "WildMain", false);
    success(&expected);
    assert_eq!(
        expected.stdout,
        fs::read(fixture("expected/wildrecv.txt")).unwrap()
    );
    for private in [false, true] {
        let out = p.join(if private { "private" } else { "jar" });
        success(&compile(&src, &out, false, private));
        let actual = run(&out, "WildMain", private);
        success(&actual);
        assert_eq!(actual.stdout, expected.stdout);
    }
    fs::remove_dir_all(p).unwrap();
}
#[test]
fn wildcard_receiver_acceptance_matches_scalac_both_directions() {
    if !Path::new(SCALAC).exists() || !Path::new(JAR).exists() {
        eprintln!("SKIP: real Scala 2.13.16 is unavailable");
        return;
    }
    let cases = [
        ("object O { var x = 1 }; object M { def f(): Unit = { import O._; x = 2 } }", true),
        ("class B { var x = 1 }; object O extends B; object M { def f(): Unit = { import O._; x = 2 } }", true),
        ("object O { val x = 1 }; object M { def f(): Unit = { import O._; x = 2 } }", false),
        ("object O { def x: Int = 1 }; object M { def f(): Unit = { import O._; x = 2 } }", false),
        ("object O { var x = 1 }; object M { def f(): Int = { import O.{x => _, _}; x } }", false),
        ("object O { private var x = 1 }; object M { def f(): Unit = { import O._; x = 2 } }", false),
    ];
    let p = temp();
    for (i, (source, accepted)) in cases.iter().enumerate() {
        let src = p.join(format!("case{i}.scala"));
        fs::write(&src, source).unwrap();
        let oracle = compile(&src, &p.join(format!("nsc{i}")), true, false);
        assert_eq!(oracle.status.success(), *accepted, "oracle case {i}");
        for private in [false, true] {
            let actual = compile(&src, &p.join(format!("rs{i}-{private}")), false, private);
            assert_eq!(
                actual.status.success(),
                oracle.status.success(),
                "case {i}, private={private}: {}",
                String::from_utf8_lossy(&actual.stderr)
            );
        }
    }
    for private in [false, true] {
        assert!(!compile(
            &fixture("wildrecv_bad.scala"),
            &p.join(format!("bad-{private}")),
            false,
            private
        )
        .status
        .success());
    }
    fs::remove_dir_all(p).unwrap();
}
