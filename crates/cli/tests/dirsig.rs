//! Directory and jar classpaths preserve the same Scala member signatures.
use std::{fs, path::Path, process::Command};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
#[test]
fn directory_and_jar_signatures_match_scalac() {
    let p = std::env::temp_dir().join(format!(
        "dirsig-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&p).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let lib = p.join("lib");
    fs::create_dir(&lib).unwrap();
    let r = Command::new("/tmp/scala-2.13.16/bin/scalac")
        .arg(fixtures.join("dirsig_lib.scala"))
        .arg("-d")
        .arg(&lib)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let jar = p.join("lib.jar");
    assert!(Command::new("jar")
        .arg("cf")
        .arg(&jar)
        .arg("-C")
        .arg(&lib)
        .arg(".")
        .status()
        .unwrap()
        .success());
    for dependency in [&lib, &jar] {
        let cp = format!("{}:{JAR}", dependency.display());
        for (source, good) in [
            ("dirsig.scala", true),
            ("dirsig_bad.scala", false),
            ("dirsig_flat_bad.scala", false),
        ] {
            for oracle in [false, true] {
                let out = p.join(format!(
                    "{}-{source}-{oracle}",
                    dependency.file_name().unwrap().to_str().unwrap()
                ));
                fs::create_dir(&out).unwrap();
                let mut c = if oracle {
                    Command::new("/tmp/scala-2.13.16/bin/scalac")
                } else {
                    let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                    c.args(["compile", "--scala-library", JAR]);
                    c
                };
                let r = c
                    .arg(fixtures.join(source))
                    .args(["-cp", &cp])
                    .arg("-d")
                    .arg(&out)
                    .output()
                    .unwrap();
                assert_eq!(
                    r.status.success(),
                    good,
                    "cp={cp} source={source} oracle={oracle}: {}",
                    String::from_utf8_lossy(&r.stderr)
                );
                if good {
                    let r = Command::new("java")
                        .args([
                            "-Xverify:all",
                            "-cp",
                            &format!("{}:{cp}", out.display()),
                            "Main",
                        ])
                        .output()
                        .unwrap();
                    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
                    assert_eq!(
                        r.stdout,
                        fs::read(fixtures.join("expected/dirsig.txt")).unwrap()
                    );
                }
            }
        }
    }
    fs::remove_dir_all(p).unwrap();
}
