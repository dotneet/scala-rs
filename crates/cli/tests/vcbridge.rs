//! Value classes crossing erased generic method bridges.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn root() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "vcbridge-{}-{}-{}",
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
fn value_class_bridge_matches_scalac() {
    check_fixture("vcbridge.scala", "expected/vcbridge.txt", &[false]);
    check_fixture(
        "vcbridge_plain.scala",
        "expected/vcbridge_plain.txt",
        &[false, true],
    );
}
fn check_fixture(src: &str, expected_file: &str, modes: &[bool]) {
    let p = root();
    let source = fixture(src);
    let oracle = p.join("nsc");
    check(&compile(&[source.clone()], &oracle, JAR, true, false));
    let expected = run(&oracle, JAR);
    check(&expected);
    assert_eq!(expected.stdout, fs::read(fixture(expected_file)).unwrap());
    for &private in modes {
        let out = p.join(if private { "private" } else { "jar" });
        check(&compile(&[source.clone()], &out, JAR, false, private));
        let actual = run(&out, JAR);
        check(&actual);
        assert_eq!(actual.stdout, expected.stdout);
    }
}

#[test]
fn generic_value_class_bridge_matches_scalac() {
    check_fixture(
        "vcbridge_generic.scala",
        "expected/vcbridge_generic.txt",
        &[false, true],
    );
}

#[test]
fn value_class_bridge_rejects_other_wrappers() {
    let p = root();
    let source = fixture("vcbridge_bad.scala");
    for (name, oracle, private) in [
        ("nsc", true, false),
        ("jar", false, false),
        ("private", false, true),
    ] {
        let result = compile(&[source.clone()], &p.join(name), JAR, oracle, private);
        assert!(
            !result.status.success(),
            "{name} accepted incompatible value classes"
        );
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("type mismatch"), "{diagnostic}");
    }
}

#[test]
fn named_value_class_bridge_collision_is_rejected() {
    let p = root();
    for (name, oracle, private) in [
        ("nsc", true, false),
        ("jar", false, false),
        ("private", false, true),
    ] {
        let output = compile(
            &[fixture("vcbridge_collision_bad.scala")],
            &p.join(name),
            JAR,
            oracle,
            private,
        );
        assert!(
            !output.status.success(),
            "{name} accepted an erased bridge collision"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("clashes"),
            "{output:?}"
        );
    }
}

#[test]
fn generic_value_class_from_scalac_binary() {
    let p = root();
    let source = fs::read_to_string(fixture("vcbridge_generic.scala")).unwrap();
    let (library, main) = source.split_once("object Main").unwrap();
    let libsrc = p.join("Library.scala");
    let client = p.join("Client.scala");
    fs::write(&libsrc, library).unwrap();
    fs::write(&client, format!("object Main{main}")).unwrap();
    let lib = p.join("lib");
    check(&compile(&[libsrc], &lib, JAR, true, false));
    let cp = format!("{}:{JAR}", lib.display());
    let nsc = p.join("nsc");
    check(&compile(&[client.clone()], &nsc, &cp, true, false));
    let expected = run(&nsc, &cp);
    check(&expected);
    for private in [false, true] {
        let out = p.join(if private { "private" } else { "jar" });
        check(&compile(&[client.clone()], &out, &cp, false, private));
        let result = run(&out, &cp);
        check(&result);
        assert_eq!(result.stdout, expected.stdout);
    }
}

#[test]
fn scalac_reads_generic_value_class_from_our_binary() {
    let p = root();
    let source = fs::read_to_string(fixture("vcbridge_generic.scala")).unwrap();
    let (library, main) = source.split_once("object Main").unwrap();
    let libsrc = p.join("Library.scala");
    let client = p.join("Client.scala");
    fs::write(&libsrc, library).unwrap();
    fs::write(&client, format!("object Main{main}")).unwrap();
    let lib = p.join("lib");
    check(&compile(&[libsrc], &lib, JAR, false, false));
    let cp = format!("{}:{JAR}", lib.display());
    let out = p.join("nsc");
    check(&compile(&[client], &out, &cp, true, false));
    let result = run(&out, &cp);
    check(&result);
    assert_eq!(
        result.stdout,
        fs::read(fixture("expected/vcbridge_generic.txt")).unwrap()
    );
}
