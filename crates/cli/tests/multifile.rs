//! Whole-run compilation: several files in one invocation, referring to each
//! other across packages.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn multi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Two tests can share a tag, and the clock is not fine enough to
    // separate them: they ran in the same directory and each `java Main` saw
    // the other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-multi-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Files naming each other across packages compile in one run and run.
#[test]
fn cross_file_references_resolve() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    if Command::new("java").arg("-version").output().is_err() {
        return;
    }
    let dir = multi_dir();
    let out = tmp_dir("cross");
    let status = Command::new(bin())
        .args([
            "compile",
            dir.join("main.scala").to_str().unwrap(),
            dir.join("lib_a.scala").to_str().unwrap(),
            dir.join("lib_b.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs");
    assert!(status.success(), "multi-file compile failed");
    let output = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Main",
        ])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let got = String::from_utf8_lossy(&output.stdout).into_owned();
    let want = "a\n6\n5\n";
    assert!(
        got.starts_with(want),
        "unexpected output: {got:?} (wanted it to start with {want:?})"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A name in an enclosing package is visible without an import -- when the
/// clause that opened it is the *nested* one (SLS 9.2). `tests/multi/
/// pkg_inner.scala` used to say `package top.inner`, which nsc 2.13.16
/// rejects (`not found: value Helper`, with and without `-Xsource:3`); it
/// compiled here only through the last-resort package walk `agent/proj` left
/// in place, and `agent/tail6` deleted that walk once the hole it covered --
/// a default argument typed at the call site -- was closed. The qualified
/// spelling is `crates/cli/tests/proj.rs`.
#[test]
fn enclosing_package_names_are_visible() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    if Command::new("java").arg("-version").output().is_err() {
        return;
    }
    let dir = multi_dir();
    let out = tmp_dir("pkg");
    let status = Command::new(bin())
        .args([
            "compile",
            dir.join("pkg_inner.scala").to_str().unwrap(),
            dir.join("pkg_outer.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs");
    assert!(status.success(), "compile failed");
    let output = Command::new("java")
        .args([
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "top.inner.Main",
        ])
        .output()
        .expect("java");
    assert!(output.status.success(), "run failed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
    let _ = fs::remove_dir_all(&out);
}

/// slick's cake pattern: an inner class declared in a component trait, mixed
/// into a profile through a self-type, and used from a leaf profile in a file
/// that comes *earlier* on the command line. Compiling in either file order
/// must give the same answer.
#[test]
fn cake_inner_classes_resolve_across_files() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    if Command::new("java").arg("-version").output().is_err() {
        return;
    }
    let dir = multi_dir();
    let leaf = dir.join("cake_profile.scala");
    let mid = dir.join("cake_relational.scala");
    let base = dir.join("cake_component.scala");
    let orders: [[&PathBuf; 3]; 2] = [[&leaf, &mid, &base], [&base, &mid, &leaf]];
    for (i, order) in orders.iter().enumerate() {
        let out = tmp_dir(&format!("cake{i}"));
        let output = Command::new(bin())
            .args([
                "compile",
                order[0].to_str().unwrap(),
                order[1].to_str().unwrap(),
                order[2].to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
                "-Xsource:3",
                "--scala-library",
                jar.to_str().unwrap(),
            ])
            .output()
            .expect("run scala-rs");
        assert!(
            output.status.success(),
            "cake compile failed (order {i}): {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let run = Command::new("java")
            .args([
                "-Xverify:all",
                "-cp",
                &format!("{}:{}", out.display(), jar.display()),
                "cake.jdbc.Main",
            ])
            .output()
            .expect("java");
        assert!(
            run.status.success(),
            "run failed (order {i}): {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "create table people\ncreate sequence ids\n100\ndb2:int\n",
            "unexpected output (order {i})"
        );
        let _ = fs::remove_dir_all(&out);
    }
}

/// The other half of the cake fix: a name that is *not* in the linearization
/// stays an error, whichever file the parent chain lives in. `Present` is
/// inherited, `Missing` exists nowhere, and `Detached` sits in a component
/// the base never mixes in.
#[test]
fn cake_names_outside_the_linearization_are_errors() {
    let dir = multi_dir();
    let out = tmp_dir("cakebad");
    let mut args = vec![
        "compile".to_string(),
        dir.join("cake_bad_leaf.scala")
            .to_str()
            .unwrap()
            .to_string(),
        dir.join("cake_bad_base.scala")
            .to_str()
            .unwrap()
            .to_string(),
        "-d".to_string(),
        out.to_str().unwrap().to_string(),
        "-Xsource:3".to_string(),
    ];
    if let Some(jar) = scala_library_jar() {
        args.push("--scala-library".to_string());
        args.push(jar.to_str().unwrap().to_string());
    }
    let output = Command::new(bin())
        .args(&args)
        .output()
        .expect("run scala-rs");
    assert!(!output.status.success(), "expected the bad cake to fail");
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(
        err.contains("not found: type Missing"),
        "missing diagnostic for `Missing`: {err}"
    );
    assert!(
        err.contains("not found: type Detached"),
        "missing diagnostic for `Detached`: {err}"
    );
    assert!(
        !err.contains("not found: type Present"),
        "`Present` is inherited and must resolve: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
