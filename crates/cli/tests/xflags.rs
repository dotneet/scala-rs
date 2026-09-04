//! Two scalac flags: `-Xsource-features:<features>` and `-Xasync`.
//!
//! `-Xsource-features` is the *behaviour* half of scalac's Scala 3 migration
//! settings (`-Xsource:3` is the warning half). nsc gates every feature on
//! `isScala3`, so `-Xsource-features` alone is dropped with a warning, and
//! `-Xsource:3-cross` is exactly `-Xsource:3 -Xsource-features:_`. The one
//! feature implemented here is `case-apply-copy-access`: the primary
//! constructor's access modifier is copied onto the synthesized `apply` and
//! `copy`, so `case class C private (x: Int)` can no longer be built through
//! `C(1)` or rebuilt through `c.copy(x = 2)`.
//!
//! `-Xasync` enables nsc's async phase. The state-machine transform is not
//! implemented (see `docs/not-implemented.md`); what *is* implemented is the
//! part a program can observe: the flag reaches a macro through
//! `c.compilerSettings`, which is where `scala.async.Async.asyncImpl`'s
//! "The async requires the compiler option -Xasync" message comes from.
//!
//! Everything below that can be is checked against real scalac 2.13.16: the
//! same fixture, the same flags, the same acceptance and the same output.

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
        "scala-rs-xflags-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn javac_available() -> bool {
    Command::new("javac")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

/// The two flags that turn the feature on, as one list.
const FEATURE: &[&str] = &["-Xsource:3", "-Xsource-features:case-apply-copy-access"];

/// Compile one fixture with scala-rs against the real scala-library.
fn compile(name: &str, out: &Path, extra: &[&str]) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library jar");
    let mut args: Vec<String> = vec![
        "compile".into(),
        fixture(name).display().to_string(),
        "-d".into(),
        out.display().to_string(),
        "--scala-library".into(),
        jar.display().to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    Command::new(bin())
        .args(&args)
        .output()
        .expect("run scala-rs compile")
}

fn compile_ok(name: &str, out: &Path, extra: &[&str]) {
    let output = compile(name, out, extra);
    assert!(
        output.status.success(),
        "expected {name} {extra:?} to compile, got:\n{}",
        diagnostics(&output)
    );
}

fn run_main(cp: &str, main: &str) -> String {
    let run = Command::new("java")
        .args(["-cp", cp, main])
        .output()
        .expect("run java");
    assert!(
        run.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

/// `javap -p` of one class, or `None` when there is no JDK to ask.
fn javap(out: &Path, class: &str) -> Option<String> {
    let o = Command::new("javap")
        .args(["-p", "-classpath", out.to_str().unwrap(), class])
        .output()
        .ok()?;
    o.status.success().then(|| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

/// Compile a fixture with real scalac. `None` when scalac is not installed.
fn scalac_compile(name: &str, out: &Path, extra: &[&str]) -> Option<std::process::Output> {
    let sc = scalac()?;
    let mut args: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    args.push("-d".into());
    args.push(out.display().to_string());
    args.push(fixture(name).display().to_string());
    Some(
        Command::new(sc)
            .args(&args)
            .output()
            .expect("run scalac compile"),
    )
}

// ------------------------------------------------ -Xsource-features: parsing

/// nsc drops `-Xsource-features` entirely below `-Xsource:3`
/// (`Global.sourceFeatures` is `isScala3 && contains(...)`), with
/// `ScalaSettings.conflictWarning`'s message. The fixture must still compile.
#[test]
fn features_without_xsource3_are_ignored_with_a_warning() {
    if scala_library_jar().is_none() {
        eprintln!("skipping: no scala-library jar");
        return;
    }
    let out = tmp_dir("conflict");
    let output = compile(
        "xflags_case_access_bad",
        &out,
        &["-Xsource-features:case-apply-copy-access"],
    );
    let err = diagnostics(&output);
    assert!(
        output.status.success(),
        "the ignored feature must not change the compile: {err}"
    );
    assert!(
        err.contains("Conflicting compiler settings were detected")
            && err.contains("-Xsource-features requires -Xsource:3"),
        "missing nsc's conflict warning: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// An unknown feature name is nsc's own error, not a silent no-op.
#[test]
fn an_unknown_feature_is_rejected() {
    let out = tmp_dir("badfeature");
    let output = compile("xflags_case_flags", &out, &["-Xsource-features:bogus"]);
    let err = diagnostics(&output);
    assert!(!output.status.success(), "expected a rejection, got {err}");
    assert!(
        err.contains("'bogus' is not a valid choice for '-Xsource-features'"),
        "unexpected message: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A feature this compiler parses but does not implement says so rather than
/// pretending. Naming a *group* (`_`, `v2.13.14`) does not warn -- that is
/// what `-Xsource:3-cross` expands to.
#[test]
fn an_unimplemented_feature_named_explicitly_warns() {
    if scala_library_jar().is_none() {
        eprintln!("skipping: no scala-library jar");
        return;
    }
    let out = tmp_dir("unimpl");
    let output = compile(
        "xflags_case_flags",
        &out,
        &["-Xsource:3", "-Xsource-features:leading-infix"],
    );
    let err = diagnostics(&output);
    assert!(output.status.success(), "expected a warning only: {err}");
    assert!(
        err.contains("-Xsource-features:leading-infix is accepted but not implemented"),
        "unexpected message: {err}"
    );

    let quiet = tmp_dir("unimpl-group");
    let output = compile("xflags_case_flags", &quiet, &["-Xsource:3-cross"]);
    assert!(
        !diagnostics(&output).contains("accepted but not implemented"),
        "a feature group must not warn per member: {}",
        diagnostics(&output)
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&quiet);
}

/// `-Xsource-features:help` lists the features and compiles nothing.
#[test]
fn features_help_lists_them() {
    let output = Command::new(bin())
        .args(["compile", "-Xsource-features:help"])
        .output()
        .expect("run scala-rs");
    let text = diagnostics(&output);
    assert!(output.status.success(), "help must succeed: {text}");
    for needle in [
        "case-apply-copy-access",
        "case-companion-function",
        "v2.13.15",
        "-Xsource:3-cross",
    ] {
        assert!(text.contains(needle), "help is missing {needle}: {text}");
    }
}

/// Both flags are in `--help`.
#[test]
fn help_mentions_both_flags() {
    let output = Command::new(bin()).arg("--help").output().expect("run");
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(
        s.contains("-Xsource-features:"),
        "help missing the flag: {s}"
    );
    assert!(s.contains("-Xasync"), "help missing -Xasync: {s}");
}

// ------------------------------------------ case-apply-copy-access: rejection

/// The point of the feature. Without it `apply` and `copy` are public and walk
/// straight around the private constructor; with it they are not.
///
/// scalac 2.13.16 rejects exactly `a`, `b`, `c` and `e` -- `D(1)` stays legal
/// because nsc copies `protected` onto `copy` but not onto `apply`, and the
/// `private[xflags]` pair stays legal because `Use` is inside `xflags`.
#[test]
fn the_feature_closes_the_private_constructor() {
    if scala_library_jar().is_none() {
        eprintln!("skipping: no scala-library jar");
        return;
    }
    let off = tmp_dir("bad-off");
    compile_ok("xflags_case_access_bad", &off, &[]);

    let on = tmp_dir("bad-on");
    let output = compile("xflags_case_access_bad", &on, FEATURE);
    let err = diagnostics(&output);
    assert!(!output.status.success(), "expected rejections: {err}");
    let errors: Vec<&str> = err.lines().filter(|l| l.starts_with("error:")).collect();
    assert_eq!(
        errors,
        vec![
            "error: value apply cannot be accessed as a member of C$ from Use$",
            "error: value apply cannot be accessed as a member of C$ from Use$",
            "error: value copy cannot be accessed as a member of C from Use$",
            "error: value copy cannot be accessed as a member of D from Use$",
        ],
        "unexpected diagnostics:\n{err}"
    );

    // The same file, the same two verdicts, from real scalac.
    if scalac().is_some() {
        let sc_off = tmp_dir("bad-sc-off");
        let o = scalac_compile("xflags_case_access_bad", &sc_off, &[]).unwrap();
        assert!(
            o.status.success(),
            "scalac rejected the fixture without the feature: {}",
            diagnostics(&o)
        );
        let sc_on = tmp_dir("bad-sc-on");
        let o = scalac_compile("xflags_case_access_bad", &sc_on, FEATURE).unwrap();
        let text = diagnostics(&o);
        assert!(!o.status.success(), "scalac accepted it with the feature");
        for needle in [
            "method apply in object C cannot be accessed",
            "method copy in class C cannot be accessed",
            "method copy in class D cannot be accessed",
        ] {
            assert!(text.contains(needle), "scalac is missing {needle}: {text}");
        }
        assert!(
            !text.contains("method apply in object D cannot be accessed"),
            "`protected` must not reach `apply`: {text}"
        );
        let _ = fs::remove_dir_all(&sc_off);
        let _ = fs::remove_dir_all(&sc_on);
    }
    let _ = fs::remove_dir_all(&off);
    let _ = fs::remove_dir_all(&on);
}

/// `-Xsource:3-cross` is `-Xsource:3 -Xsource-features:_`, so it turns the
/// feature on; `-Xsource:3` on its own does not.
#[test]
fn xsource3_cross_implies_the_feature_and_xsource3_alone_does_not() {
    if scala_library_jar().is_none() {
        eprintln!("skipping: no scala-library jar");
        return;
    }
    let plain = tmp_dir("x3");
    compile_ok("xflags_case_access_bad", &plain, &["-Xsource:3"]);

    let cross = tmp_dir("x3cross");
    let output = compile("xflags_case_access_bad", &cross, &["-Xsource:3-cross"]);
    assert!(
        !output.status.success(),
        "-Xsource:3-cross must imply case-apply-copy-access: {}",
        diagnostics(&output)
    );
    let _ = fs::remove_dir_all(&plain);
    let _ = fs::remove_dir_all(&cross);
}

// ------------------------------------------ case-apply-copy-access: classfile

/// The feature is marked `[bin]` in nsc's own help because it changes the
/// class file. Nothing in this fixture *uses* `apply` or `copy`, so no
/// reference forces the widening both compilers apply to a `private` member
/// read from another class file, and the access flags can be compared
/// straight across.
#[test]
fn the_feature_changes_the_class_file_the_way_scalac_does() {
    if scala_library_jar().is_none() {
        eprintln!("skipping: no scala-library jar");
        return;
    }
    let off = tmp_dir("flags-off");
    compile_ok("xflags_case_flags", &off, &[]);
    let on = tmp_dir("flags-on");
    compile_ok("xflags_case_flags", &on, FEATURE);

    let Some(off_c) = javap(&off, "xflags.C$") else {
        eprintln!("skipping the class-file half: no javap");
        return;
    };
    assert!(
        off_c.contains("public xflags.C apply(int)")
            && off_c.contains("extends scala.runtime.AbstractFunction1"),
        "the default must be unchanged: {off_c}"
    );

    let on_c = javap(&on, "xflags.C$").expect("javap C$");
    assert!(
        on_c.contains("private xflags.C apply(int)"),
        "`private` constructor: apply must be private: {on_c}"
    );
    assert!(
        !on_c.contains("AbstractFunction1"),
        "a companion with a non-public apply is not a FunctionN: {on_c}"
    );
    let on_cc = javap(&on, "xflags.C").expect("javap C");
    assert!(
        on_cc.contains("private xflags.C copy(int)"),
        "`private` constructor: copy must be private: {on_cc}"
    );

    // `protected` reaches `copy` at the Scala level but is a public method in
    // the class file, and never reaches `apply` at all.
    let on_d = javap(&on, "xflags.D$").expect("javap D$");
    assert!(
        on_d.contains("public xflags.D apply(int)") && on_d.contains("AbstractFunction1"),
        "`protected` must leave apply alone: {on_d}"
    );

    // `private[xflags]` is a public method, but still costs the FunctionN.
    let on_e = javap(&on, "xflags.E$").expect("javap E$");
    assert!(
        on_e.contains("public xflags.E apply(int)"),
        "a qualified private is public in the class file: {on_e}"
    );
    assert!(
        !on_e.contains("AbstractFunction1"),
        "a qualified private still costs the FunctionN parent: {on_e}"
    );

    // And the plain case class is untouched.
    let on_f = javap(&on, "xflags.F$").expect("javap F$");
    assert!(
        on_f.contains("public xflags.F apply(int)") && on_f.contains("AbstractFunction1"),
        "a plain case class must be unchanged: {on_f}"
    );

    // The same four verdicts from real scalac, on the same file.
    if scalac().is_some() {
        let sc = tmp_dir("flags-sc");
        let o = scalac_compile("xflags_case_flags", &sc, FEATURE).unwrap();
        assert!(o.status.success(), "scalac: {}", diagnostics(&o));
        for (class, needle, want) in [
            ("xflags.C$", "private xflags.C apply(int)", true),
            ("xflags.C$", "AbstractFunction1", false),
            ("xflags.C", "private xflags.C copy(int)", true),
            ("xflags.D$", "AbstractFunction1", true),
            ("xflags.E$", "AbstractFunction1", false),
            ("xflags.F$", "AbstractFunction1", true),
        ] {
            let text = javap(&sc, class).expect("javap scalac output");
            assert_eq!(
                text.contains(needle),
                want,
                "scalac {class}: expected contains({needle}) == {want}, got {text}"
            );
        }
        let _ = fs::remove_dir_all(&sc);
    }
    let _ = fs::remove_dir_all(&off);
    let _ = fs::remove_dir_all(&on);
}

/// A `private` `apply` / `copy` still has to *run*. Both compilers widen the
/// member when it is read from another class file -- nsc renames it as well
/// (`C$$copy`), this one only widens -- and the program prints the same thing
/// with the feature on, with it off, and under scalac.
#[test]
fn a_private_apply_and_copy_still_run() {
    if scala_library_jar().is_none() || !java_available() {
        eprintln!("skipping: no scala-library jar or no java");
        return;
    }
    let jar = scala_library_jar().unwrap();
    let expected = expected_stdout("xflags_case_access");

    for extra in [&[][..], FEATURE] {
        let out = tmp_dir("rt");
        compile_ok("xflags_case_access", &out, extra);
        let cp = format!("{}:{}", out.display(), jar.display());
        assert_eq!(
            run_main(&cp, "Main"),
            expected,
            "wrong output with {extra:?}"
        );
        let _ = fs::remove_dir_all(&out);
    }

    if scalac().is_some() {
        let sc = tmp_dir("rt-sc");
        let o = scalac_compile("xflags_case_access", &sc, FEATURE).unwrap();
        assert!(o.status.success(), "scalac: {}", diagnostics(&o));
        let cp = format!("{}:{}", sc.display(), jar.display());
        assert_eq!(run_main(&cp, "Main"), expected, "scalac disagrees");
        let _ = fs::remove_dir_all(&sc);
    }
}

// ------------------------------------------------------------------- -Xasync

/// `-Xasync` reaches a macro implementation through `c.compilerSettings`,
/// which is where scala-async's "The async requires the compiler option
/// -Xasync" comes from. Two compilations, because a macro implementation must
/// come from an earlier run.
#[test]
fn xasync_reaches_a_macro_through_compiler_settings() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skipping: no scala-library / scala-reflect jar");
        return;
    };
    if !java_available() || !javac_available() {
        eprintln!("skipping: the macro engine needs java and javac");
        return;
    }
    let impl_out = tmp_dir("async-impl");
    compile_ok(
        "xflags_async_impl",
        &impl_out,
        &["-cp", reflect.to_str().unwrap()],
    );
    let cp = format!("{}:{}", impl_out.display(), reflect.display());

    // Without the flag the macro aborts with scala-async's own message.
    let no_flag = tmp_dir("async-off");
    let output = compile("xflags_async_use", &no_flag, &["-cp", &cp]);
    let err = diagnostics(&output);
    assert!(!output.status.success(), "expected the gate to fire: {err}");
    assert!(
        err.contains(
            "The async requires the compiler option -Xasync \
             (supported only by Scala 2.12.12+ / 2.13.3+)"
        ),
        "unexpected message: {err}"
    );

    // With it, the macro expands and the program runs.
    let with_flag = tmp_dir("async-on");
    compile_ok("xflags_async_use", &with_flag, &["-cp", &cp, "-Xasync"]);
    let run_cp = format!("{}:{}", with_flag.display(), jar.display());
    assert_eq!(
        run_main(&run_cp, "Main"),
        expected_stdout("xflags_async_use")
    );

    // Real scalac, the same two files, the same two verdicts.
    if scalac().is_some() {
        let sc_impl = tmp_dir("async-sc-impl");
        let o = scalac_compile("xflags_async_impl", &sc_impl, &[]).unwrap();
        assert!(o.status.success(), "scalac impl: {}", diagnostics(&o));
        let sc_cp = sc_impl.display().to_string();

        let sc_off = tmp_dir("async-sc-off");
        let o = scalac_compile("xflags_async_use", &sc_off, &["-cp", &sc_cp]).unwrap();
        let text = diagnostics(&o);
        assert!(!o.status.success(), "scalac accepted it without -Xasync");
        assert!(
            text.contains("The async requires the compiler option -Xasync"),
            "scalac said something else: {text}"
        );

        let sc_on = tmp_dir("async-sc-on");
        let o = scalac_compile("xflags_async_use", &sc_on, &["-cp", &sc_cp, "-Xasync"]).unwrap();
        assert!(o.status.success(), "scalac -Xasync: {}", diagnostics(&o));
        let cp = format!("{}:{}", sc_on.display(), jar.display());
        assert_eq!(run_main(&cp, "Main"), expected_stdout("xflags_async_use"));
        let _ = fs::remove_dir_all(&sc_impl);
        let _ = fs::remove_dir_all(&sc_off);
        let _ = fs::remove_dir_all(&sc_on);
    }
    let _ = fs::remove_dir_all(&impl_out);
    let _ = fs::remove_dir_all(&no_flag);
    let _ = fs::remove_dir_all(&with_flag);
}
