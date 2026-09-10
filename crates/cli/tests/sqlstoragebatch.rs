//! Constructor loading, real Slick interpolation and storage metadata matrix.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const REFLECT: &str = "/tmp/scala-2.13.16/lib/scala-reflect.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let r = std::env::temp_dir().join(format!(
        "sqlstorage-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&r).unwrap();
    r
}
fn compile(name: &str, nsc: bool, out: &Path, cp: &str, accepted: bool) {
    fs::create_dir(out).unwrap();
    let mut c = Command::new(if nsc {
        NSC
    } else {
        env!("CARGO_BIN_EXE_scala-rs")
    });
    if !nsc {
        c.args(["compile", "--scala-library", JAR]);
    }
    let p = c
        .arg(fixtures().join(format!("{name}.scala")))
        .args(["-cp", cp, "-d"])
        .arg(out)
        .output()
        .unwrap();
    assert_eq!(
        p.status.success(),
        accepted,
        "{name} nsc={nsc} out={}: {}",
        out.display(),
        String::from_utf8_lossy(&p.stderr)
    );
    if !accepted {
        let s = String::from_utf8_lossy(&p.stderr);
        let reason = if name == "sqlstorage_sql_bad" {
            s.contains("SetParameter")
        } else if name.starts_with("sqlstorage_access_bad") {
            s.contains("cannot be accessed") || s.contains("not a member")
        } else {
            s.contains("overload") || s.contains("type mismatch")
        };
        assert!(reason, "wrong rejection: {s}");
    }
}
fn run(out: &Path, cp: &str) -> Vec<u8> {
    run_main(out, cp, "Main")
}
fn run_main(out: &Path, cp: &str, main: &str) -> Vec<u8> {
    let p = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            main,
        ])
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    p.stdout
}
fn expected(name: &str) -> Vec<u8> {
    fs::read(fixtures().join("expected").join(format!("{name}.txt"))).unwrap()
}
#[test]
fn java_string_constructor_families_match_scalac() {
    let r = root();
    let mut reference = None;
    for nsc in [true, false] {
        let out = r.join(format!("use-{nsc}"));
        compile("sqlstorage_strings", nsc, &out, JAR, true);
        let stdout = run(&out, JAR);
        assert_eq!(stdout, expected("sqlstorage_strings"));
        assert_eq!(&stdout, reference.get_or_insert_with(|| stdout.clone()));
        compile(
            "sqlstorage_strings_bad",
            nsc,
            &r.join(format!("bad-{nsc}")),
            JAR,
            false,
        );
    }
    fs::remove_dir_all(r).unwrap();
}
#[test]
fn storage_and_constructor_argument_flags_match_both_compilers() {
    let r = root();
    let cp = format!("{JAR}:{REFLECT}");
    let mut reference = None;
    for producer in [true, false] {
        let api = r.join(format!("api-{producer}"));
        compile("sqlstorage_api", producer, &api, &cp, true);
        let cp = format!("{}:{cp}", api.display());
        for consumer in [true, false] {
            let out = r.join(format!("use-{producer}-{consumer}"));
            compile("sqlstorage_use", consumer, &out, &cp, true);
            let stdout = run(&out, &cp);
            assert_eq!(stdout, expected("sqlstorage_use"));
            assert_eq!(&stdout, reference.get_or_insert_with(|| stdout.clone()));
            for case in ["hidden", "plain", "private", "mutable"] {
                compile(
                    &format!("sqlstorage_access_bad_{case}"),
                    consumer,
                    &r.join(format!("bad-{case}-{producer}-{consumer}")),
                    &cp,
                    false,
                );
            }
        }
    }
    fs::remove_dir_all(r).unwrap();
}
#[test]
fn slick_sql_interpolation_executes_parameter_binding() {
    let r = root();
    let helper = r.join("helper");
    fs::create_dir(&helper).unwrap();
    let p = Command::new("javac")
        .arg("-d")
        .arg(&helper)
        .arg(fixtures().join("SqlStorageParameters.java"))
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let cache = PathBuf::from(std::env::var("HOME").unwrap())
        .join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2");
    let deps = [
        "com/typesafe/slick/slick_2.13/3.4.1/slick_2.13-3.4.1.jar",
        "com/typesafe/config/1.4.9/config-1.4.9.jar",
        "org/reactivestreams/reactive-streams/1.0.4/reactive-streams-1.0.4.jar",
        "org/slf4j/slf4j-api/2.0.18/slf4j-api-2.0.18.jar",
    ];
    let mut cp = format!("{}:{JAR}:{REFLECT}", helper.display());
    for dep in deps {
        let p = cache.join(dep);
        assert!(p.is_file(), "missing {}", p.display());
        cp.push(':');
        cp.push_str(p.to_str().unwrap());
    }
    let mut reference = None;
    for nsc in [true, false] {
        let out = r.join(format!("use-{nsc}"));
        compile("sqlstorage_sql", nsc, &out, &cp, true);
        let stdout = run(&out, &cp);
        assert_eq!(stdout, expected("sqlstorage_sql"));
        assert_eq!(&stdout, reference.get_or_insert_with(|| stdout.clone()));
        compile(
            "sqlstorage_sql_bad",
            nsc,
            &r.join(format!("bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(r).unwrap();
}

#[test]
fn macro_argument_types_cover_literals_and_repeated_splices() {
    let r = root();
    let api = r.join("api");
    let cp = format!("{JAR}:{REFLECT}");
    compile("sqlstorage_macro_api", true, &api, &cp, true);
    let cp = format!("{}:{cp}", api.display());
    let mut reference = None;
    for nsc in [true, false] {
        let out = r.join(format!("use-{nsc}"));
        compile("sqlstorage_macro_use", nsc, &out, &cp, true);
        let stdout = run(&out, &cp);
        assert_eq!(stdout, expected("sqlstorage_macro_use"));
        assert_eq!(&stdout, reference.get_or_insert_with(|| stdout.clone()));
        compile(
            "sqlstorage_macro_bad",
            nsc,
            &r.join(format!("bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(r).unwrap();
}

#[test]
fn qualified_access_and_value_class_defaults_survive_separate_compilation() {
    let r = root();
    for (case, main) in [
        ("qualified", "sqlstoragequalified.Main"),
        ("value_default", "Main"),
        ("accessor_abi", "Main"),
    ] {
        let mut reference = None;
        for producer in [true, false] {
            let api = r.join(format!("{case}-api-{producer}"));
            compile(&format!("sqlstorage_{case}_api"), producer, &api, JAR, true);
            let cp = format!("{}:{JAR}", api.display());
            for consumer in [true, false] {
                let out = r.join(format!("{case}-use-{producer}-{consumer}"));
                compile(&format!("sqlstorage_{case}_use"), consumer, &out, &cp, true);
                if case == "accessor_abi" {
                    compile(
                        "sqlstorage_access_bad_captured",
                        consumer,
                        &r.join(format!("{case}-bad-captured-{producer}-{consumer}")),
                        &cp,
                        false,
                    );
                    compile(
                        "sqlstorage_access_bad_value",
                        consumer,
                        &r.join(format!("{case}-bad-{producer}-{consumer}")),
                        &cp,
                        false,
                    );
                }
                let stdout = run_main(&out, &cp, main);
                assert_eq!(stdout, expected(&format!("sqlstorage_{case}_use")));
                assert_eq!(&stdout, reference.get_or_insert_with(|| stdout.clone()));
            }
        }
    }
    fs::remove_dir_all(r).unwrap();
}
