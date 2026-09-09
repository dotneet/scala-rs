//! Outer type families through nested API conversions, from source and pickle.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn root() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "retfamily-{}-{}-{}",
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
fn outer_type_family_matches_scalac_from_source_and_binary() {
    if !Path::new(NSC).is_file() || !Path::new(JAR).is_file() {
        eprintln!("SKIP: real Scala 2.13.16 is unavailable");
        return;
    }
    let p = root();
    let original = fs::read_to_string(fixture("retfamily_lib.scala")).unwrap();
    for shape in ["chain", "diamond", "reverse"] {
        let diamond = shape != "chain";
        let dir = p.join(shape);
        fs::create_dir(&dir).unwrap();
        let libsrc = dir.join("Family.scala");
        let text = if diamond {
            original.replace("class Derived extends Root {\n  type Out[A] = Rich[A]", "trait Left extends Root { type Out[A] = Rich[A]; def make[A](x: A): Rich[A] = new Rich(x) }\ntrait Right extends Root\nclass Derived extends Left with Right {")
                .replace("  def make[A](x: A): Rich[A] = new Rich(x)\n", "")
        } else {
            original.clone()
        };
        let text = if shape == "reverse" {
            text.replace("extends Left with Right", "extends Right with Left")
        } else {
            text
        };
        fs::write(&libsrc, text).unwrap();
        let lib = dir.join("library");
        check(&compile(
            std::slice::from_ref(&libsrc),
            &lib,
            JAR,
            true,
            false,
        ));
        let cp = format!("{}:{JAR}", lib.display());
        let app = fixture("retfamily.scala");
        let oracle = dir.join("oracle");
        check(&compile(
            std::slice::from_ref(&app),
            &oracle,
            &cp,
            true,
            false,
        ));
        let expected = run(&oracle, &cp);
        check(&expected);
        assert_eq!(
            expected.stdout,
            fs::read(fixture("expected/retfamily.txt")).unwrap()
        );
        let bad = fixture("retfamily_bad.scala");
        assert!(!compile(
            std::slice::from_ref(&bad),
            &dir.join("oracle-bad"),
            &cp,
            true,
            false
        )
        .status
        .success());
        for binary in [false, true] {
            for private in [false, true] {
                let out = dir.join(format!("app-{binary}-{private}"));
                let sources = if binary {
                    vec![app.clone()]
                } else {
                    vec![libsrc.clone(), app.clone()]
                };
                let classpath = if binary { cp.as_str() } else { JAR };
                check(&compile(&sources, &out, classpath, false, private));
                let actual = run(&out, classpath);
                check(&actual);
                assert_eq!(
                    actual.stdout, expected.stdout,
                    "diamond={diamond} binary={binary} private={private}"
                );
                let bad_sources = if binary {
                    vec![bad.clone()]
                } else {
                    vec![libsrc.clone(), bad.clone()]
                };
                assert!(!compile(
                    &bad_sources,
                    &dir.join(format!("bad-{binary}-{private}")),
                    classpath,
                    false,
                    private
                )
                .status
                .success());
            }
        }
    }
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn collection_overloads_keep_the_selected_jvm_declaration() {
    collection_fixture_matches_scalac("retfamily_collections");
}

#[test]
fn fixed_key_maps_keep_or_change_their_key_type() {
    collection_fixture_matches_scalac("retfamily_fixedkeys");
}

#[test]
fn collection_element_results_keep_the_receiver_constructor() {
    collection_fixture_matches_scalac("retfamily_elements");
}

fn collection_fixture_matches_scalac(name: &str) {
    if !Path::new(NSC).is_file() || !Path::new(JAR).is_file() {
        eprintln!("SKIP: real Scala 2.13.16 is unavailable");
        return;
    }
    let p = root();
    let good = [fixture(&format!("{name}.scala"))];
    let bad = [fixture(&format!("{name}_bad.scala"))];
    let mut oracle_stdout = None;
    for oracle in [true, false] {
        let out = p.join(if oracle { "nsc" } else { "rs" });
        check(&compile(&good, &out, JAR, oracle, false));
        let result = run(&out, JAR);
        check(&result);
        let expected = fs::read(fixture(&format!("expected/{name}.txt"))).unwrap();
        assert_eq!(result.stdout, expected);
        if let Some(ref expected) = oracle_stdout {
            assert_eq!(&result.stdout, expected);
        } else {
            oracle_stdout = Some(result.stdout);
        }
        let rejected = compile(&bad, &out.join("bad"), JAR, oracle, false);
        assert!(
            !rejected.status.success(),
            "invalid collection result was accepted"
        );
    }
}
