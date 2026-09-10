//! Expected results constrain partially applied factories before implicit search.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn root() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "partfactory-{}-{}-{}",
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
fn receiver_parameters_match_scalac_from_source_and_binary() {
    let p = root();
    let lib = p.join("lib");
    check(&compile(
        &[fixture("partfactory_lib.scala")],
        &lib,
        JAR,
        true,
        false,
    ));
    let expected = fs::read(fixture("expected/partfactory.txt")).unwrap();
    for binary in [false, true] {
        let cp = if binary {
            format!("{}:{JAR}", lib.display())
        } else {
            JAR.to_string()
        };
        for bad in [false, true] {
            let mut files = vec![fixture(if bad {
                "partfactory_bad.scala"
            } else {
                "partfactory.scala"
            })];
            if !binary {
                files.push(fixture("partfactory_lib.scala"));
            }
            for oracle in [false, true] {
                let out = p.join(format!("{binary}-{bad}-{oracle}"));
                let r = compile(&files, &out, &cp, oracle, false);
                assert_eq!(
                    r.status.success(),
                    !bad,
                    "binary={binary} bad={bad} oracle={oracle}: {}",
                    String::from_utf8_lossy(&r.stderr)
                );
                if !bad {
                    let r = run(&out, &cp);
                    check(&r);
                    assert_eq!(r.stdout, expected);
                }
            }
        }
    }
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn receiver_constraints_reject_conflicts_and_bound_violations() {
    let p = root();
    for (i, rhs) in [
        "val x: Cell[String] = Factory.make(1)",
        "val x: Cell[String] = Factory.bounded(\"bad\")",
        "val x = Factory.bounded(\"bad\")",
    ]
    .iter()
    .enumerate()
    {
        let source = p.join(format!("Bad{i}.scala"));
        fs::write(&source, format!("object Bad {{ {rhs} }}")).unwrap();
        for oracle in [false, true] {
            let out = p.join(format!("{i}-{oracle}"));
            let r = compile(
                &[source.clone(), fixture("partfactory_lib.scala")],
                &out,
                JAR,
                oracle,
                false,
            );
            assert!(
                !r.status.success(),
                "invalid constraint accepted: {rhs}, oracle={oracle}"
            );
        }
    }
    fs::remove_dir_all(p).unwrap();
}
