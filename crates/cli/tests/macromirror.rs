//! Reverse RPC: a macro implementation asking the *running compiler* a
//! question in the middle of its own expansion. `docs/macros.md` §7.20.
//!
//! Everything else the engine does it can do inside
//! `scala.reflect.runtime.universe`, looking things up on the macro classpath.
//! `c.typecheck` cannot: what a tree means depends on the scope the macro was
//! called from, and that scope only ever existed inside scala-rs. So the
//! expansion protocol became a conversation -- the engine writes `(q …)`,
//! scala-rs answers `(a …)` and goes back to waiting for the expansion -- and
//! the answer is computed by really typing the tree, in the real typer, at the
//! real call site.
//!
//! Two compilations, because nsc requires two: a macro implementation has to
//! come from an *earlier* run. `mtc_impl.scala` is compiled first,
//! `mtc_use.scala` second with the first one's output on the classpath.
//!
//! The check that matters is the **dual run**: real scalac 2.13.16 compiles
//! the same two files against each other and the two programs must print the
//! same thing. A `c.typecheck` that answered plausibly but wrongly would still
//! compile and still run; only the output would differ.

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
        "scala-rs-macromirror-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn zio_jars() -> Option<Vec<PathBuf>> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let relative = [
        "dev/zio/zio_2.13/2.1.26/zio_2.13-2.1.26.jar",
        "dev/zio/zio-internal-macros_2.13/2.1.26/zio-internal-macros_2.13-2.1.26.jar",
        "dev/zio/zio-stacktracer_2.13/2.1.26/zio-stacktracer_2.13-2.1.26.jar",
        "dev/zio/izumi-reflect_2.13/3.0.9/izumi-reflect_2.13-3.0.9.jar",
        "dev/zio/izumi-reflect-thirdparty-boopickle-shaded_2.13/3.0.9/izumi-reflect-thirdparty-boopickle-shaded_2.13-3.0.9.jar",
        "org/scala-lang/modules/scala-collection-compat_2.13/2.14.0/scala-collection-compat_2.13-2.14.0.jar",
    ];
    [
        home.join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        home.join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ]
    .into_iter()
    .find_map(|root| {
        let jars: Vec<_> = relative.iter().map(|path| root.join(path)).collect();
        jars.iter().all(|path| path.is_file()).then_some(jars)
    })
}

fn required_assets() -> bool {
    std::env::var_os("SCALA_RS_REQUIRE_TEST_ASSETS").is_some()
}

fn skip_or_panic(tag: &str, why: &str) -> bool {
    if required_assets() {
        panic!("required assets for {tag} are missing: {why}");
    }
    eprintln!("skip {tag}: {why}");
    false
}

fn prerequisites(tag: &str) -> bool {
    if !tool_available("java") || !tool_available("javac") {
        return skip_or_panic(tag, "java / javac not available");
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() {
        return skip_or_panic(tag, "scala-library / scala-reflect not obtainable");
    }
    true
}

#[test]
fn java_macro_protocol_parser_rejects_malformed_frames() {
    if !tool_available("java") || !tool_available("javac") {
        skip_or_panic("macro protocol self-test", "java / javac not available");
        return;
    }
    let out = tmp_dir("protocol-engine");
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../typer/java/ScalaRsMacroEngine.java");
    let compile = Command::new("javac")
        .args(["--release", "8", "-d"])
        .arg(&out)
        .arg(&source)
        .output()
        .expect("compile protocol self-test engine");
    assert!(
        compile.status.success(),
        "javac protocol self-test engine failed: {}",
        diagnostics(&compile)
    );
    let run = Command::new("java")
        .args([
            "-cp",
            out.to_str().unwrap(),
            "ScalaRsMacroEngine",
            "--protocol-self-test",
        ])
        .output()
        .expect("run protocol self-test engine");
    assert!(
        run.status.success(),
        "protocol self-test failed: {}",
        diagnostics(&run)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "ok\n");
    let _ = fs::remove_dir_all(out);
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

/// Compile the named fixtures with scala-rs, against scala-reflect.jar plus
/// `extra`.
fn compile(names: &[&str], out: &Path, extra: &[&Path]) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library");
    let reflect = scala_reflect_jar().expect("scala-reflect");
    let mut cp = reflect.display().to_string();
    for e in extra {
        cp.push(':');
        cp.push_str(&e.display().to_string());
    }
    let mut command = Command::new(bin());
    command.arg("compile");
    for name in names {
        command.arg(fixtures_dir().join(format!("{name}.scala")));
    }
    command
        .args(["-d", out.to_str().unwrap(), "-cp", &cp, "--scala-library"])
        .arg(&jar)
        .output()
        .expect("run scala-rs compile")
}

fn run_main(cp: &str, what: &str) -> String {
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for {what}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The whole reverse channel, end to end: six macros that ask `c.typecheck`
/// something, expanded for real and executed, against real scalac's output.
///
/// Two of the seven lines are the mirror over the current run's own symbols:
/// `Marker` is a class *this compilation is defining*, so it has no class file
/// and the engine's `mirror.staticClass` could never find it. It reaches the
/// engine described -- parents, declarations and their types -- because
/// scala-rs was asked, at the moment the name had to be handed over.
#[test]
fn mtc_typecheck_expands_and_runs() {
    if !prerequisites("mtc_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("impl");
    let uses = tmp_dir("use");

    let out = compile(&["mtc_impl"], &impls, &[]);
    assert!(
        out.status.success(),
        "compile mtc_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile(&["mtc_use"], &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile mtc_use failed: {}",
        diagnostics(&out)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected/mtc_use.txt")).unwrap();
    assert_eq!(run_main(&cp, "mtc_use"), expected, "stdout mismatch");
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// `inferImplicitValue` is answered by the call-site implicit scope, including
/// a stable path-dependent target, and `withMacrosDisabled` removes the
/// companion macro without hiding an ordinary local witness.
#[test]
fn infer_implicit_value_matches_real_scalac() {
    if !prerequisites("miv_use") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        skip_or_panic("miv_use oracle", "scalac 2.13.16 not available");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("miv-impl");
    let uses = tmp_dir("miv-use");
    let scalac_uses = tmp_dir("miv-scalac-use");

    // The macro implementation is a real scalac classpath artifact, as ZIO's
    // stack-tracer macros are. Both compilers consume the identical classes.
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("miv_impl.scala"))
        .output()
        .expect("compile miv_impl with scalac");
    assert!(
        out.status.success(),
        "scalac miv_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile(&["miv_use"], &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile miv_use failed: {}",
        diagnostics(&out)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", impls.display(), reflect.display()),
            "-d",
            scalac_uses.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("miv_use.scala"))
        .output()
        .expect("compile miv_use with scalac");
    assert!(
        out.status.success(),
        "scalac miv_use failed: {}",
        diagnostics(&out)
    );

    let ours_cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    let scalac_cp = format!(
        "{}:{}:{}:{}",
        scalac_uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    let ours = run_main(&ours_cp, "miv_use scala-rs");
    let oracle = run_main(&scalac_cp, "miv_use scalac");
    assert_eq!(ours, oracle, "scala-rs differs from scalac");
    assert_eq!(
        ours,
        fs::read_to_string(fixtures_dir().join("expected/miv_use.txt")).unwrap()
    );

    for dir in [impls, uses, scalac_uses] {
        let _ = fs::remove_dir_all(dir);
    }
}

/// Exercise the exact ZIO 2.1.26 stack-tracer macro artifact that motivated
/// this RPC, rather than only the structurally equivalent local fixture.
#[test]
fn actual_zio_auto_trace_macro_expands() {
    if !prerequisites("actual ZIO autoTrace") {
        return;
    }
    let Some(jars) = zio_jars() else {
        skip_or_panic("actual ZIO autoTrace", "ZIO 2.1.26 jars not available");
        return;
    };
    let out_dir = tmp_dir("miv-zio-actual");
    let extra: Vec<_> = jars.iter().map(PathBuf::as_path).collect();
    let out = compile(&["miv_zio_actual"], &out_dir, &extra);
    assert!(
        out.status.success(),
        "compile against actual ZIO stack-tracer macro failed: {}",
        diagnostics(&out)
    );
    assert!(out_dir.join("Main$.class").is_file());
    let _ = fs::remove_dir_all(out_dir);
}

/// Reverse implicit queries must fail closed when answering faithfully would
/// require re-entering the busy macro engine or erasing a local singleton
/// prefix or uses a non-default diagnostic position. A non-silent miss, on
/// the other hand, is an ordinary compiler diagnostic. Real scalac establishes
/// the oracle side of all four cases.
#[test]
fn infer_implicit_value_adversarial_queries_fail_closed() {
    if !prerequisites("miv adversarial") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        skip_or_panic("miv adversarial oracle", "scalac 2.13.16 not available");
        return;
    };
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("miv-adversarial-impl");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("miv_impl.scala"))
        .output()
        .expect("compile miv_impl with scalac");
    assert!(
        out.status.success(),
        "scalac miv_impl failed: {}",
        diagnostics(&out)
    );

    let scalac_enabled = tmp_dir("miv-scalac-enabled");
    let scalac_nonstatic = tmp_dir("miv-scalac-nonstatic");
    let scalac_position = tmp_dir("miv-scalac-position");
    for (name, out_dir) in [
        ("miv_enabled", &scalac_enabled),
        ("miv_nonstatic", &scalac_nonstatic),
        ("miv_position", &scalac_position),
    ] {
        let out = Command::new(&scalac)
            .args([
                "-cp",
                &format!("{}:{}", impls.display(), reflect.display()),
                "-d",
                out_dir.to_str().unwrap(),
            ])
            .arg(fixtures_dir().join(format!("{name}.scala")))
            .output()
            .expect("compile adversarial oracle with scalac");
        assert!(
            out.status.success(),
            "scalac {name} failed: {}",
            diagnostics(&out)
        );
    }

    let enabled_out = tmp_dir("miv-enabled");
    let out = compile(&["miv_enabled"], &enabled_out, &[&impls]);
    let text = diagnostics(&out);
    assert!(!out.status.success(), "enabled implicit macro was accepted");
    assert!(
        text.contains("selected implicit macro `automatic`")
            && text.contains("could not expand it while answering the outer macro"),
        "wrong enabled-macro refusal: {text}"
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("error:"))
            .count(),
        1
    );

    let position_out = tmp_dir("miv-position");
    let out = compile(&["miv_position"], &position_out, &[&impls]);
    let text = diagnostics(&out);
    assert!(
        !out.status.success(),
        "non-default implicit position was accepted"
    );
    assert!(
        text.contains("c.inferImplicitValue with a non-default `pos`"),
        "wrong non-default-position refusal: {text}"
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("error:"))
            .count(),
        1
    );

    let nonstatic_out = tmp_dir("miv-nonstatic");
    let out = compile(&["miv_nonstatic"], &nonstatic_out, &[&impls]);
    let text = diagnostics(&out);
    assert!(!out.status.success(), "non-static type prefix was accepted");
    assert!(
        text.contains("the type `token.Type`") && text.contains("cannot rebuild at the call site"),
        "wrong non-static-prefix refusal: {text}"
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("error:"))
            .count(),
        1
    );

    let required_out = tmp_dir("miv-required");
    let out = compile(&["miv_required"], &required_out, &[&impls]);
    let text = diagnostics(&out);
    assert!(!out.status.success(), "silent=false miss was accepted");
    assert!(
        text.contains("could not find implicit value of type MivMissing"),
        "wrong silent=false diagnostic: {text}"
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("error:"))
            .count(),
        1
    );

    let scalac_required = tmp_dir("miv-scalac-required");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", impls.display(), reflect.display()),
            "-d",
            scalac_required.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("miv_required.scala"))
        .output()
        .expect("compile silent=false oracle with scalac");
    let oracle = diagnostics(&out);
    assert!(!out.status.success(), "scalac accepted silent=false miss");
    assert!(
        oracle.contains("TypecheckException: implicit search has failed"),
        "unexpected scalac silent=false diagnostic: {oracle}"
    );

    for dir in [
        impls,
        scalac_enabled,
        scalac_nonstatic,
        scalac_position,
        enabled_out,
        nonstatic_out,
        position_out,
        required_out,
        scalac_required,
    ] {
        let _ = fs::remove_dir_all(dir);
    }
}

/// Every question the mirror cannot answer is refused *by name*.
///
/// This is the half that keeps the other half honest. Four of these six call
/// sites are programs real scalac 2.13.16 compiles and runs (printing
/// `Int(1) / Int(1) / Int(1) / caught`); scala-rs answers none of them,
/// and each refusal says which capability was missing. An approximate answer
/// would compile here and be wrong, and nothing downstream would notice --
/// which is exactly what `docs/macros.md` §7.18 warns a half-built mirror
/// does.
#[test]
fn mtc_unanswerable_questions_are_named() {
    if !prerequisites("mtc_bad") {
        return;
    }
    let impls = tmp_dir("bad-impl");
    let uses = tmp_dir("bad-use");

    let out = compile(&["mtc_impl", "mtc_bad_impl"], &impls, &[]);
    assert!(
        out.status.success(),
        "compile mtc_bad_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile(&["mtc_bad"], &uses, &[&impls]);
    let text = diagnostics(&out);
    for want in [
        // `withImplicitViewsDisabled` / `withMacrosDisabled`: a typer mode
        // scala-rs has no switch for, named rather than ignored.
        "asked to disable implicit views, which scala-rs's typer has no switch for",
        "asked to disable macro expansion, which scala-rs's typer has no switch for",
        // An expected type asks a different question from the one that is sent.
        "was given the expected type `Any`",
        // PATTERNmode, named rather than read as TERMmode.
        "was asked for PATTERNmode, which scala-rs does not implement",
        // A `TypecheckException` cannot cross a `java.lang.reflect.Proxy`, so
        // an implementation that catches one did not see what nsc shows it.
        "scala-rs cannot hand a TypecheckException to an implementation",
        // A node the reply rebuilder has no scala-rs tree for, outside the
        // pattern it could stand in.
        "contains a `Star`, which scala-rs cannot rebuild yet",
        // A class with a `val` used to be the mirror's limit here (`class
        // Bag(val size: Int)`, "`size` is a field"). The mirror now answers in
        // nsc's shape, field and accessor both, so that call site left this
        // file; `crates/cli/tests/gbmac.rs` compares such answers with real
        // scalac's and pins the refusals that remain.
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    // Every call site is an error, and none of them is accepted.
    assert!(!out.status.success() || text.contains("error:"), "{text}");
    assert_eq!(
        text.matches("macro expansion is not implemented").count(),
        6,
        "every call site must be refused:\n{text}"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}
