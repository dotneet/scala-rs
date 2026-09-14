//! A Scala classpath reader must preserve a generic accessor's stable path.
//!
//! The library and consumer are compiled separately so the consumer reads
//! `Config[P].profile` from classfiles/pickle, then uses `c.profile` both as a
//! wildcard-import prefix and as a singleton type prefix.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn compile(src: &Path, out: &Path, jar: &Path, cp: Option<&Path>) {
    let cp = cp.map(|path| path.to_string_lossy().into_owned());
    compile_with_classpath(src, out, jar, cp.as_deref());
}

fn compile_with_classpath(src: &Path, out: &Path, jar: &Path, cp: Option<&str>) {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    if let Some(cp) = cp {
        cmd.args(["-cp", cp]);
    }
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {} failed:\n{}{}",
        src.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn slick_jar() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let root = PathBuf::from(home).join(
        "Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/com/typesafe/slick/slick_2.13",
    );
    for version in ["3.6.1", "3.4.1"] {
        let jar = root.join(version).join(format!("slick_2.13-{version}.jar"));
        if jar.is_file() {
            return Some(jar);
        }
    }
    None
}

#[test]
fn generic_profile_accessor_is_a_stable_path_after_separate_compilation() {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !jar.is_file() {
        eprintln!("skip stable profile path: scala-library jar not obtainable");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "scala-rs-stable-profile-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let lib = root.join("lib");
    let use_ = root.join("use");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&use_).unwrap();

    compile(
        &fixtures_dir().join("stable_profile_lib.scala"),
        &lib,
        &jar,
        None,
    );
    compile(
        &fixtures_dir().join("stable_profile_use.scala"),
        &use_,
        &jar,
        Some(&lib),
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inherited_profile_accessor_preserves_a_narrowed_api_bound() {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !jar.is_file() {
        eprintln!("skip inherited profile API: scala-library jar not obtainable");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "scala-rs-inherited-profile-api-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    compile(
        &fixtures_dir().join("profile_api_inherited.scala"),
        &root,
        &jar,
        None,
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn abstract_profile_upper_bound_reconnects_after_a_producer_consumer_boundary() {
    let library = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    let Some(slick) = slick_jar() else {
        eprintln!("skip abstract profile API: slick jar not obtainable");
        return;
    };
    if !library.is_file() {
        eprintln!("skip abstract profile API: scala-library jar not obtainable");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "scala-rs-abstract-profile-api-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let producer = root.join("producer");
    let consumer = root.join("consumer");
    fs::create_dir_all(&producer).unwrap();
    fs::create_dir_all(&consumer).unwrap();

    // The producer's Profile bound is recorded in ScalaSignature, while the
    // JVM accessor descriptor only carries JdbcProfile's erasure. The
    // consumer must therefore reconnect the fully-qualified upper bound after
    // JdbcProfile is loaded from the separate Slick jar.
    compile_with_classpath(
        &fixtures_dir().join("slick_abstract_profile_lib.scala"),
        &producer,
        &library,
        Some(slick.to_str().unwrap()),
    );
    let consumer_cp = format!("{}:{}", producer.display(), slick.display());
    compile_with_classpath(
        &fixtures_dir().join("slick_abstract_profile_use.scala"),
        &consumer,
        &library,
        Some(&consumer_cp),
    );

    // Real scalac accepts the same producer/consumer boundary and all four
    // factory shapes in the fixture: explicit type arguments, inferred
    // `TableQuery(new Users(_))`, an object parent and an anonymous subclass.
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if scalac.is_file() {
        let scalac_consumer = root.join("scalac-consumer");
        let use_src = fixtures_dir().join("slick_abstract_profile_use.scala");
        fs::create_dir_all(&scalac_consumer).unwrap();
        let output = Command::new(&scalac)
            .args([
                "-cp",
                &consumer_cp,
                "-d",
                scalac_consumer.to_str().unwrap(),
                use_src.to_str().unwrap(),
            ])
            .output()
            .expect("run real scalac");
        assert!(
            output.status.success(),
            "real scalac rejected abstract profile factory fixture:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        eprintln!("skip abstract profile scalac comparison: scalac 2.13.16 not obtainable");
    }

    fs::remove_dir_all(root).unwrap();
}
