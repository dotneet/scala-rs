//! An `implicit class` read back out of a `-cp` pickle is still implicit.
//!
//! nsc expands `implicit class C(x: T) { … }` into a plain `class C` plus a
//! conversion method `implicit def C(x: T): C`, and it marks that method
//! **`SYNTHETIC`**. `Member::is_public_api` filtered `SYNTHETIC` out, so every
//! `implicit class` any library on `-cp` declares was dropped on the way in.
//! The class file cannot stand in for the pickle here: nothing in bytecode
//! records `implicit`, so what the class-file reader installs is an ordinary
//! method — in scope under its own name, callable explicitly, and never
//! selectable as a view. `C(x).m` compiled; `x.m` did not.
//!
//! The second half is the nesting. A *nested* class has no `ScalaSignature` of
//! its own (its pickle sits on the enclosing top-level class file), and
//! nothing adopts a class the program never writes by name — which
//! `import <a val>._` never does. `Typer::import_wildcard` now adopts the
//! class it is about to walk instead of waiting for something else to.
//!
//! Both were measured on gitbucket, whose `blocking-slick` dependency declares
//! ten `implicit class`es in a `BlockingAPI` trait nested inside
//! `BlockingJdbcProfile`: `value withTransaction is not a member of
//! DatabaseDef` (13), `withSession` (7), `run` on `Rep[Boolean]` (5) and
//! `firstOption` (1) are all one root, and all 26 go.
//!
//! The check that settles it is `ic_app_matches_scalac`: **real scalac
//! 2.13.16** compiles `ic_lib.scala`, so the pickle read back is nsc's own,
//! scala-rs compiles `ic_app.scala` against those class files, and the pair
//! runs and prints exactly what scalac-on-scalac prints for the same two
//! sources.

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

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-implclassbin-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_scalac(scalac: &Path, args: &[&str]) {
    let output = Command::new(scalac)
        .args(args)
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A generic value class erases to its wrapped field after substituting the
/// class type argument. Binary implicit discovery must compare that resulting
/// descriptor rather than treating the field as `Object`.
#[test]
fn generic_value_class_implicit_bridge_matches_scalac() {
    let (Some(jar), Some(scalac_bin)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip generic value class: needs scala-library and scalac 2.13.16");
        return;
    };
    if !java_available() {
        return;
    }
    let api_src = fixtures_dir().join("generic_valueclass_implicit_api.scala");
    let app_src = fixtures_dir().join("generic_valueclass_implicit_app.scala");
    let api = tmp_dir("generic-valueclass-api");
    run_scalac(
        &scalac_bin,
        &["-d", api.to_str().unwrap(), api_src.to_str().unwrap()],
    );

    let nsc_app = tmp_dir("generic-valueclass-nsc-app");
    run_scalac(
        &scalac_bin,
        &[
            "-cp",
            api.to_str().unwrap(),
            "-d",
            nsc_app.to_str().unwrap(),
            app_src.to_str().unwrap(),
        ],
    );
    let control = run_main(&format!(
        "{}:{}:{}",
        api.display(),
        nsc_app.display(),
        jar.display()
    ));

    let rs_app = tmp_dir("generic-valueclass-rs-app");
    let output = Command::new(bin())
        .args([
            "compile",
            app_src.to_str().unwrap(),
            "-d",
            rs_app.to_str().unwrap(),
            "-cp",
            api.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs could not resolve the generic value-class bridge:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ours = run_main(&format!(
        "{}:{}:{}",
        api.display(),
        rs_app.display(),
        jar.display()
    ));
    assert_eq!(ours, control);
    assert_eq!(control, "ok\n");

    for d in [api, nsc_app, rs_app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// A ScalaSignature on a scala-rs module class makes its implicit vals visible
/// once during the eager directory classpath scan and again when the companion
/// pickle is supplied on demand. The eager term must be replaced by the
/// precise pickled method, not left beside it.
#[test]
fn rs_implicit_val_directory_classpath_is_not_duplicated() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implicit val: needs the scala-library jar");
        return;
    };
    if !java_available() {
        return;
    }
    let lib_src = fixtures_dir().join("implicit_val_lib.scala");
    let app_src = fixtures_dir().join("implicit_val_app.scala");
    let lib = tmp_dir("implicit-val-lib");
    let output = Command::new(bin())
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "-d",
            lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs library compile");
    assert!(
        output.status.success(),
        "scala-rs could not compile implicit_val_lib.scala:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let app = tmp_dir("implicit-val-app");
    let output = Command::new(bin())
        .args([
            "compile",
            app_src.to_str().unwrap(),
            "-d",
            app.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs application compile");
    assert!(
        output.status.success(),
        "scala-rs duplicated the implicit val when reading a directory classpath:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let result = run_main(&format!(
        "{}:{}:{}",
        lib.display(),
        app.display(),
        jar.display()
    ));
    assert_eq!(result, "ok\n");
    let _ = fs::remove_dir_all(lib);
    let _ = fs::remove_dir_all(app);
}

/// scalac compiles `ic_lib.scala`; both compilers then take `ic_app.scala`
/// against those class files, and the two programs print the same thing.
#[test]
fn ic_app_matches_scalac() {
    let (Some(jar), Some(scalac_bin)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip ic_app: needs the scala-library jar and scalac 2.13.16");
        return;
    };
    if !java_available() {
        return;
    }
    let lib_src = fixtures_dir().join("ic_lib.scala");
    let app_src = fixtures_dir().join("ic_app.scala");

    // The library half is nsc's, always: the defect is in *reading* the
    // pickle nsc writes, so compiling it with scala-rs would not exercise it.
    let lib = tmp_dir("lib");
    run_scalac(
        &scalac_bin,
        &["-d", lib.to_str().unwrap(), lib_src.to_str().unwrap()],
    );

    // Control: scalac compiles the app half too.
    let nsc_app = tmp_dir("nsc-app");
    run_scalac(
        &scalac_bin,
        &[
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            nsc_app.to_str().unwrap(),
            app_src.to_str().unwrap(),
        ],
    );
    let control = run_main(&format!(
        "{}:{}:{}",
        lib.display(),
        nsc_app.display(),
        jar.display()
    ));

    // The real thing: scala-rs compiles the app half.
    let rs_app = tmp_dir("rs-app");
    let output = Command::new(bin())
        .args([
            "compile",
            app_src.to_str().unwrap(),
            "-d",
            rs_app.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs could not compile ic_app.scala against scalac's ic_lib:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ours = run_main(&format!(
        "{}:{}:{}",
        lib.display(),
        rs_app.display(),
        jar.display()
    ));

    assert_eq!(
        ours, control,
        "scala-rs and scalac disagree on ic_app.scala's output"
    );
    // Pinned, so a change that makes *both* wrong in the same way is still a
    // failure: `bump` twice, `wide` twice, the explicit application, `shout`.
    assert_eq!(control, "42\nwide41\n42\nwide41\n42\nhello!\n");

    for d in [lib, nsc_app, rs_app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// A classfile accessor inherited from a classpath trait carries its exact
/// JVM return type only in the method descriptor. The eager classpath scan
/// must retain that identity as a loadable class symbol when the type is not
/// visible by simple name; otherwise a later selection sees an inert
/// `Type::Named` and cannot load the external API's members.
#[test]
fn classpath_descriptor_accessor_loads_external_members() {
    let (Some(jar), Some(scalac_bin)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip classpath descriptor: needs scala-library and scalac 2.13.16");
        return;
    };
    if !java_available() {
        return;
    }

    let api_src = fixtures_dir().join("classpath_descriptor_api.scala");
    let holder_src = fixtures_dir().join("classpath_descriptor_holder.scala");
    let app_src = fixtures_dir().join("classpath_descriptor_app.scala");

    // The API is nsc-produced and packaged separately, as it is in the
    // original minimal reproduction. Its package is intentionally not open
    // in the consumer, so simple-name lookup cannot resolve `Api`.
    let api_jar = tmp_dir("descriptor-api").with_extension("jar");
    run_scalac(
        &scalac_bin,
        &["-d", api_jar.to_str().unwrap(), api_src.to_str().unwrap()],
    );

    // Compile the accessor-bearing library with scala-rs. The raw
    // `Base.api(): Ldescriptor/Api;` descriptor is the path under test.
    let holder = tmp_dir("descriptor-holder");
    let output = Command::new(bin())
        .args([
            "compile",
            holder_src.to_str().unwrap(),
            "-d",
            holder.to_str().unwrap(),
            "-cp",
            api_jar.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs holder compile");
    assert!(
        output.status.success(),
        "scala-rs could not compile the descriptor holder:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    // Reading the holder directory is the regression: `Holder.api` must be a
    // class-symbol-backed `descriptor.Api`, so the consumer can select
    // `feature` from the external classpath jar.
    let app = tmp_dir("descriptor-app");
    let cp = format!("{}:{}", holder.display(), api_jar.display());
    let output = Command::new(bin())
        .args([
            "compile",
            app_src.to_str().unwrap(),
            "-d",
            app.to_str().unwrap(),
            "-cp",
            &cp,
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs descriptor app compile");
    assert!(
        output.status.success(),
        "scala-rs could not select a member from the external descriptor type:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let _ = fs::remove_file(api_jar);
    let _ = fs::remove_dir_all(holder);
    let _ = fs::remove_dir_all(app);
}

#[test]
fn synthetic_conversions_in_unmodeled_scala_packages_are_imported() {
    let (Some(jar), Some(oracle)) = (scala_library_jar(), scalac()) else {
        return;
    };
    let root = tmp_dir("namespace");
    let lib = root.join("lib");
    fs::create_dir(&lib).unwrap();
    run_scalac(
        &oracle,
        &[
            "-d",
            lib.to_str().unwrap(),
            fixtures_dir()
                .join("ic_namespace_lib.scala")
                .to_str()
                .unwrap(),
        ],
    );
    let cp = format!("{}:{}", lib.display(), jar.display());
    for native in [false, true] {
        let out = root.join(format!("app-{native}"));
        fs::create_dir(&out).unwrap();
        let mut cmd = Command::new(if native { bin() } else { oracle.clone() });
        if native {
            cmd.arg("compile");
        }
        let result = cmd
            .arg(fixtures_dir().join("ic_namespace_app.scala"))
            .args(["-cp", &cp, "-d"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "native={native}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            run_main(&format!("{}:{cp}", out.display())),
            "[record]\n3000\n"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn binary_generic_by_name_parameter_is_recovered_before_overload_selection() {
    let (Some(jar), Some(oracle)) = (scala_library_jar(), scalac()) else {
        return;
    };
    if !java_available() {
        return;
    }
    let root = tmp_dir("binary-byname");
    let lib = root.join("lib");
    fs::create_dir(&lib).unwrap();
    run_scalac(
        &oracle,
        &[
            "-d",
            lib.to_str().unwrap(),
            fixtures_dir()
                .join("binary_byname_api.scala")
                .to_str()
                .unwrap(),
        ],
    );
    let cp = format!("{}:{}", lib.display(), jar.display());
    for native in [false, true] {
        let out = root.join(format!("app-{native}"));
        fs::create_dir(&out).unwrap();
        let mut cmd = Command::new(if native { bin() } else { oracle.clone() });
        if native {
            cmd.arg("compile");
        }
        let result = cmd
            .arg(fixtures_dir().join("binary_byname_app.scala"))
            .args(["-cp", &cp, "-d"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "native={native}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(run_main(&format!("{}:{cp}", out.display())), "42:1\n");
    }
    fs::remove_dir_all(root).unwrap();
}
