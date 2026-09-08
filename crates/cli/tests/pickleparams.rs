//! `def f(): T` is not `def f: T`, and a class file has to say which.
//!
//! nsc pickles the first as a `MethodType` over an empty parameter list and
//! the second as a `NullaryMethodType` (a `POLYtpe` with no type parameters),
//! and it holds every call site to the difference in both directions: `f()`
//! against a `NullaryMethodType` is `Int does not take parameters`, and a
//! `MethodType` over an empty list may be called either way.
//!
//! scala-rs wrote both as the `NullaryMethodType`, so real scalac reading our
//! class files rejected the parentheses our own source had written:
//!
//! ```text
//! error: Empty does not take parameters
//!     println(Keys.empty())
//! error: Empty.type does not take parameters
//!     println(Empty())
//! ```
//!
//! Nothing about that is case-class-specific -- a zero-field case class's
//! `apply()` is just the most visible instance -- so the fix is in the
//! pickler, not the case-class synthesizer. The same write also lost the
//! *empty* clause of `def f()(implicit e: E)`, because `uncurry` joins a
//! method's clauses onto one list long before the backend sees the symbol and
//! nsc's pickler runs *before* uncurry; `uncurry` now records the shape it is
//! about to destroy (`Symbol::pickle_clauses`) and the pickler writes one
//! `METHODtpe` per clause.
//!
//! The test that settles it is `nsc_caller_uses_our_empty_parameter_lists`:
//! scala-rs compiles `pp_lib.scala`, real scalac 2.13.16 compiles
//! `pp_app.scala` against those class files, and the pair runs and prints what
//! scalac-on-scalac prints. On the binary before the fix that scalac run
//! reported 14 errors.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-pickleparams-{tag}-{}-{nanos}-{seq}",
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

fn compile_ours(name: &str, out: &Path, extra: &[&str]) {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        fixture(name).to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs compile {name} extra={extra:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
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

/// scalac's diagnostics, with the fixture's absolute path trimmed off so two
/// runs over the same source are comparable.
fn scalac_diagnostics(scalac: &Path, args: &[&str]) -> String {
    let output = Command::new(scalac)
        .args(args)
        .output()
        .expect("run scalac");
    let mut all = String::from_utf8_lossy(&output.stderr).into_owned();
    all.push_str(&String::from_utf8_lossy(&output.stdout));
    let dir = fixtures_dir()
        .canonicalize()
        .unwrap_or_else(|_| fixtures_dir());
    all.replace(&format!("{}/", dir.display()), "")
}

fn run_main(cp: &str, main: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// scala-rs compiles the library, real scalac compiles a caller that writes
/// the parentheses, and the pair runs and prints what scalac-on-scalac prints.
#[test]
fn nsc_caller_uses_our_empty_parameter_lists() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!(
            "skip empty-parameter-list interop: needs the scala-library jar and scalac 2.13.16"
        );
        return;
    };
    if !java_available() {
        return;
    }
    let lib = fixture("pp_lib");
    let app = fixture("pp_app");

    // Control: scalac compiles both halves.
    let n_lib = tmp_dir("nsc-lib");
    let n_app = tmp_dir("nsc-app");
    run_scalac(
        &scalac,
        &["-d", n_lib.to_str().unwrap(), lib.to_str().unwrap()],
    );
    run_scalac(
        &scalac,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "-d",
            n_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let control = run_main(
        &format!("{}:{}:{}", n_lib.display(), n_app.display(), jar.display()),
        "Main",
    );
    assert!(control.contains("Empty()"), "control output: {control}");

    // The real thing: scala-rs compiles the library, scalac the caller.
    let r_lib = tmp_dir("rs-lib");
    let r_app = tmp_dir("nsc-over-rs");
    compile_ours(
        "pp_lib",
        &r_lib,
        &["--scala-library", jar.to_str().unwrap()],
    );
    // `-deprecation` is deliberate: a member pickled with an empty list where
    // nsc writes a nullary one still *compiles*, with an "Auto-application to
    // `()` is deprecated" warning. That is how the over-correction shows up,
    // and `-Xfatal-warnings` makes it a failure instead of a footnote. It is
    // exactly what `copy$default$1` did before it was made nullary.
    run_scalac(
        &scalac,
        &[
            "-deprecation",
            "-Xfatal-warnings",
            "-cp",
            r_lib.to_str().unwrap(),
            "-d",
            r_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let ours = run_main(
        &format!("{}:{}:{}", r_lib.display(), r_app.display(), jar.display()),
        "Main",
    );

    assert_eq!(
        ours, control,
        "a caller real scalac compiled against our class files does not behave \
         like one compiled against scalac's own"
    );
    for d in [n_lib, n_app, r_lib, r_app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// The other half: a `def f: T` we pickled has to *stay* parameterless.
///
/// Over-correcting is as wrong as collapsing. scalac rejects `f()` against a
/// `NullaryMethodType`, and it has to reject it against ours at the same lines
/// with the same messages.
#[test]
fn nsc_rejects_parens_on_our_parameterless_defs() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip parameterless negative: needs the scala-library jar and scalac 2.13.16");
        return;
    };
    let lib = fixture("pp_lib");
    let bad = fixture("pp_app_bad");

    let n_lib = tmp_dir("neg-nsc-lib");
    run_scalac(
        &scalac,
        &["-d", n_lib.to_str().unwrap(), lib.to_str().unwrap()],
    );
    let n_out = tmp_dir("neg-nsc-out");
    let control = scalac_diagnostics(
        &scalac,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "-d",
            n_out.to_str().unwrap(),
            bad.to_str().unwrap(),
        ],
    );
    assert!(
        control.contains("does not take parameters") && control.contains("2 errors"),
        "control did not reject the parentheses: {control}"
    );

    let r_lib = tmp_dir("neg-rs-lib");
    compile_ours(
        "pp_lib",
        &r_lib,
        &["--scala-library", jar.to_str().unwrap()],
    );
    let r_out = tmp_dir("neg-rs-out");
    let ours = scalac_diagnostics(
        &scalac,
        &[
            "-cp",
            r_lib.to_str().unwrap(),
            "-d",
            r_out.to_str().unwrap(),
            bad.to_str().unwrap(),
        ],
    );

    assert_eq!(
        ours, control,
        "scalac rejects `f()` against scalac's parameterless defs but not \
         against ours (or not the same way)"
    );
    for d in [n_lib, n_out, r_lib, r_out] {
        let _ = fs::remove_dir_all(d);
    }
}

/// The reading direction: an empty parameter list in a *scalac*-compiled class
/// file has to arrive as one, callable both as `f()` and as `f`.
#[test]
fn we_read_nscs_empty_parameter_lists() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip reading nsc's empty lists: needs the scala-library jar and scalac 2.13.16");
        return;
    };
    if !java_available() {
        return;
    }
    let lib = fixture("pp_nsc_lib");
    let app = fixture("pp_ours");

    let n_lib = tmp_dir("read-nsc-lib");
    run_scalac(
        &scalac,
        &["-d", n_lib.to_str().unwrap(), lib.to_str().unwrap()],
    );

    // Control: scalac compiles the caller too.
    let n_app = tmp_dir("read-nsc-app");
    run_scalac(
        &scalac,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "-d",
            n_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let control = run_main(
        &format!("{}:{}:{}", n_lib.display(), n_app.display(), jar.display()),
        "Main",
    );

    // Ours reads the same class files.
    let r_app = tmp_dir("read-rs-app");
    compile_ours(
        "pp_ours",
        &r_app,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ],
    );
    let ours = run_main(
        &format!("{}:{}:{}", n_lib.display(), r_app.display(), jar.display()),
        "Main",
    );

    assert_eq!(
        ours, control,
        "our caller does not behave like scalac's against the same class files"
    );
    for d in [n_lib, n_app, r_app] {
        let _ = fs::remove_dir_all(d);
    }
}

/// And our own reader over our own writer: the pickle we wrote has to say the
/// same thing to us that it says to scalac.
#[test]
fn our_own_reader_round_trips_our_empty_parameter_lists() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip our own round trip: needs the scala-library jar and scalac 2.13.16");
        return;
    };
    if !java_available() {
        return;
    }
    let app = fixture("pp_app_self");

    let r_lib = tmp_dir("self-lib");
    compile_ours(
        "pp_lib",
        &r_lib,
        &["--scala-library", jar.to_str().unwrap()],
    );
    let r_app = tmp_dir("self-app");
    compile_ours(
        "pp_app_self",
        &r_app,
        &[
            "-cp",
            r_lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ],
    );
    let ours = run_main(
        &format!("{}:{}:{}", r_lib.display(), r_app.display(), jar.display()),
        "Main",
    );

    let n_lib = tmp_dir("self-control-lib");
    let n_app = tmp_dir("self-control-app");
    run_scalac(
        &scalac,
        &[
            "-d",
            n_lib.to_str().unwrap(),
            fixture("pp_lib").to_str().unwrap(),
        ],
    );
    run_scalac(
        &scalac,
        &[
            "-cp",
            n_lib.to_str().unwrap(),
            "-d",
            n_app.to_str().unwrap(),
            app.to_str().unwrap(),
        ],
    );
    let control = run_main(
        &format!("{}:{}:{}", n_lib.display(), n_app.display(), jar.display()),
        "Main",
    );

    assert_eq!(
        ours, control,
        "our own reader disagrees with our own writer"
    );
    for d in [r_lib, r_app, n_lib, n_app] {
        let _ = fs::remove_dir_all(d);
    }
}
