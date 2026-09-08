//! Type tags that are **type constructors applied to type arguments**, and
//! the `Expr[Nothing]` nsc really hands a macro implementation.
//! `docs/macros.md` §7.21.
//!
//! A tag descriptor on the wire used to be a bare class name, so a tag for
//! `List[Int]` or `ClassTag[Row]` could not be *requested* at all -- which is
//! where every one of gitbucket's 31 `mapTo` call sites stopped, one layer
//! before the macro implementation was ever invoked.
//!
//! Two compilations, because nsc requires two: a macro implementation has to
//! come from an *earlier* run. `gbm_impl.scala` is compiled first,
//! `gbm_use.scala` second with the first one's output on the classpath.
//!
//! The check that matters is the **dual run**: real scalac 2.13.16 compiles
//! the same two files against each other and the two programs must print the
//! same thing. A tag that carried a type constructor without its arguments --
//! or with the wrong ones -- would still compile and still run; only the
//! output would differ.

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
        "scala-rs-gbmapto-{tag}-{}-{nanos}-{seq}",
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

fn prerequisites(tag: &str) -> bool {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() {
        eprintln!("skip {tag}: scala-library / scala-reflect not obtainable");
        return false;
    }
    true
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

/// Nine tags whose types carry arguments, expanded for real and executed.
///
/// The first line is slick's `ShapedValue.mapToImpl` as far as it reads its
/// type argument -- `isCaseClass`, then every case accessor's
/// `typeSignature` -- driven by a tag scala-rs could not previously build.
/// Its leading `Nothing` is not a mistake: nsc hands an implementation
/// `Expr[Nothing](arg)(TypeTag.Nothing)` for every value argument, so
/// `staticType` is `Nothing` there too, and scala-rs now says the same.
#[test]
fn gbm_applied_tags_expand_and_run() {
    if !prerequisites("gbm_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("impl");
    let uses = tmp_dir("use");

    let out = compile(&["gbm_impl"], &impls, &[]);
    assert!(
        out.status.success(),
        "compile gbm_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile(&["gbm_use"], &uses, &[&impls]);
    assert!(
        out.status.success(),
        "compile gbm_use failed: {}",
        diagnostics(&out)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected/gbm_use.txt")).unwrap();
    assert_eq!(run_main(&cp, "gbm_use"), expected, "stdout mismatch");
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// What real scalac 2.13.16 makes of the same two files.
///
/// Eight of the ten lines are byte-identical. The two that are not are a
/// **named limit, pinned here rather than discovered later**: `Either` is
/// `scala.package.Either` and `Map` is `Predef.Map`, and both are type
/// *aliases*. scala-rs expands an alias away long before a type reaches the
/// tag descriptor, so its tag names the class and nsc's names the alias. The
/// two types are the same type -- they answer `=:=` and every question an
/// implementation asks of them identically -- but nsc's printer omits the
/// prefix of an alias owned by `scala` or `Predef` and does not omit
/// `scala.util.` or `scala.collection.immutable.`, so they print differently.
///
/// An implementation that *prints* or string-matches a tag therefore sees a
/// different spelling under scala-rs. That is a real difference and it is
/// stated here in full rather than hidden by dropping the two cases from the
/// fixture.
#[test]
fn gbm_applied_tags_match_real_scalac() {
    if !prerequisites("gbm_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip gbm_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("impl-scalac");
    let uses = tmp_dir("use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            impls.to_str().unwrap(),
            fixtures_dir().join("gbm_impl.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected gbm_impl.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", reflect.display(), impls.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("gbm_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected gbm_use.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        impls.display(),
        reflect.display(),
        jar.display()
    );
    let ours = fs::read_to_string(fixtures_dir().join("expected/gbm_use.txt")).unwrap();
    let want = ours
        .replace(
            "scala.util.Either[String,List[Int]] = scala.util.Either[String, List[Int]]",
            "Either[String,List[Int]] = Either[String, List[Int]]",
        )
        .replace(
            "scala.collection.immutable.Map[String,List[Int]] = \
             scala.collection.immutable.Map[String, List[Int]]",
            "Map[String,List[Int]] = Map[String, List[Int]]",
        );
    assert_eq!(
        run_main(&cp, "gbm_use (real scalac build)"),
        want,
        "recorded expectation for gbm_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// Every tag scala-rs cannot build is refused **by name**.
///
/// This is the half that keeps the other half honest. All four of these call
/// sites are a program real scalac 2.13.16 compiles and runs, printing
///
/// ```text
/// Nothing v:Int
/// LocalBox[Int] = LocalBox[Int]
/// T = T[]
/// Main.type = Main.type[]
/// ```
///
/// The first is gitbucket's `mapTo` in miniature and the reason this slice
/// does not close it: `LocalRow` is a case class **this run is compiling**, so
/// it travels as the empty placeholder of `docs/macros.md` §5.1, and the
/// implementation's verdict on a symbol carrying nothing but a name says
/// nothing about the program -- so it is replaced rather than repeated.
#[test]
fn gbm_unbuildable_tags_are_named() {
    if !prerequisites("gbm_bad") {
        return;
    }
    let impls = tmp_dir("bad-impl");
    let uses = tmp_dir("bad-use");

    let out = compile(&["gbm_impl"], &impls, &[]);
    assert!(
        out.status.success(),
        "compile gbm_impl failed: {}",
        diagnostics(&out)
    );
    let out = compile(&["gbm_bad"], &uses, &[&impls]);
    let text = diagnostics(&out);
    for want in [
        // The placeholder: a type *argument* that is a class this run is
        // compiling still carries a name and nothing else.
        "the type argument `LocalRow` is a class this run is compiling",
        // A current-run class *applied* to type arguments: the placeholder has
        // nothing for them to bind to, so it is refused rather than sent as a
        // name the engine's mirror would fail to resolve.
        "`LocalBox`, a class this run is compiling applied to type arguments",
        // A bare type parameter. nsc materialises a `WeakTypeTag` with a free
        // type; scala-rs has no such thing.
        "`T`, an abstract type with no tag in scope",
        // A singleton type has no `staticClass` to rebuild it from.
        "`Main.type`, a singleton type",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(!out.status.success() || text.contains("error:"), "{text}");
    assert_eq!(
        text.matches("macro expansion is not implemented").count(),
        4,
        "every call site must be refused:\n{text}"
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}
