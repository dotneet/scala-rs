//! Source type identities/bounds and bidirectional binary implicit evidence.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "typeidentitybatch-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}
fn compile(name: &str, mode: &str, out: &Path, cp: &str, accepted: bool) {
    compile_many(&[name], mode, out, cp, accepted);
}
fn compile_many(names: &[&str], mode: &str, out: &Path, cp: &str, accepted: bool) {
    let name = names[0];
    fs::create_dir(out).unwrap();
    let mut cmd = if mode == "nsc" {
        Command::new(NSC)
    } else {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
        c.arg("compile");
        if mode == "private" {
            c.arg("--no-scala-library");
        } else {
            c.args(["--scala-library", JAR]);
        }
        c
    };
    for name in names {
        cmd.arg(fixtures().join(format!("{name}.scala")));
    }
    cmd.arg("-d").arg(out);
    if !cp.is_empty() {
        cmd.args(["-cp", cp]);
    }
    let r = cmd.output().unwrap();
    assert_eq!(
        r.status.success(),
        accepted,
        "{name}/{mode}/{cp}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    if name == "implicitidentityambiguous_bad" {
        assert!(
            String::from_utf8_lossy(&r.stderr).contains("ambiguous"),
            "{}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
}
fn run(name: &str, mode: &str, out: &Path, cp: &str) {
    let mut classpath = out.display().to_string();
    if mode != "private" {
        classpath.push(':');
        classpath.push_str(JAR);
    }
    if !cp.is_empty() {
        classpath.push(':');
        classpath.push_str(cp);
    }
    let r = Command::new("java")
        .args(["-Xverify:all", "-cp", &classpath, "Main"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "{name}/{mode}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert_eq!(
        r.stdout,
        fs::read(fixtures().join(format!("expected/{name}.txt"))).unwrap(),
        "{name}/{mode}"
    );
}
#[test]
fn type_identity_and_written_bounds_match_scalac() {
    let root = root();
    for name in [
        "typeidentitybatch",
        "boundidentitybatch",
        "typeidentitymissing_bad",
        "typeidentitypackage_bad",
        "typeidentityarrayarity_bad",
        "typeidentitysqlarray_bad",
        "typeidentityupper_bad",
        "typeidentitylower_bad",
        "typeidentitydependent_bad",
        "typeidentitywildsibling_bad",
        "typeidentityalias_bad",
        "typeidentityaliasextra_bad",
    ] {
        let good = !name.ends_with("_bad");
        for mode in ["nsc", "jar", "private"] {
            let out = root.join(format!("{name}-{mode}"));
            compile(name, mode, &out, "", good);
            if good {
                run(name, mode, &out, "");
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn binary_implicit_evidence_round_trips_directories_and_jars() {
    let root = root();
    for producer in ["nsc", "jar"] {
        let api = root.join(format!("api-{producer}"));
        compile("implicitidentityapi", producer, &api, "", true);
        let jarfile = root.join(format!("api-{producer}.jar"));
        let packed = Command::new("jar")
            .arg("cf")
            .arg(&jarfile)
            .arg("-C")
            .arg(&api)
            .arg(".")
            .output()
            .unwrap();
        assert!(
            packed.status.success(),
            "{}",
            String::from_utf8_lossy(&packed.stderr)
        );
        for (format, cp) in [("dir", &api), ("jar", &jarfile)] {
            let cp = cp.display().to_string();
            for name in [
                "implicitidentitybatch",
                "implicitidentityambiguous_bad",
                "implicitidentityprivate_bad",
            ] {
                let good = !name.ends_with("_bad");
                for consumer in ["nsc", "jar"] {
                    let out = root.join(format!("{producer}-{format}-{name}-{consumer}"));
                    compile(name, consumer, &out, &cp, good);
                    if good {
                        run(name, consumer, &out, &cp);
                    }
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_recompilation_replaces_binary_implicit_values() {
    let root = root();
    for mode in ["nsc", "jar"] {
        let old = root.join(format!("old-{mode}"));
        compile("implicitidentityreplace1", mode, &old, "", true);
        let out = root.join(format!("new-{mode}"));
        compile(
            "implicitidentityreplace2",
            mode,
            &out,
            &old.display().to_string(),
            true,
        );
        run(
            "implicitidentityreplace2",
            mode,
            &out,
            &old.display().to_string(),
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn written_bounds_use_declaration_imports_in_both_source_orders() {
    let root = root();
    for bad in [false, true] {
        let name = if bad {
            "typeidentityheaders_bad"
        } else {
            "typeidentityheaders"
        };
        for reversed in [false, true] {
            let mut names = vec![name, "typeidentityheadersapi"];
            if reversed {
                names.reverse();
            }
            for mode in ["nsc", "jar", "private"] {
                let out = root.join(format!("{bad}-{reversed}-{mode}"));
                compile_many(&names, mode, &out, "", !bad);
                if !bad {
                    run(name, mode, &out, "");
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
