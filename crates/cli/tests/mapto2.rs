//! The type arguments written on a macro **implementation reference**.
//! `docs/macros.md` §7.22.
//!
//! `def mapTo[R] = macro ShapedValue.mapToImpl[R, U]` is slick's, and it is the
//! shape every one of gitbucket's 31 `mapTo` call sites has: `R` is `mapTo`'s
//! own type parameter and `U` is the *class's*, so the implementation asks for
//! two `WeakTypeTag`s where the call site writes one type argument. Lining the
//! two up cannot work, and nsc does not try -- it reads
//! `MacroImplBinding.targs`, the type arguments on the reference itself, and
//! resolves each one on its own. scala-rs used to peel those away and throw
//! them out, in both readers (`PickleReader::macro_impl_of` and
//! `macros.rs::split_type_apply`).
//!
//! Three compilations, because nsc requires two and this needs both readers:
//! `mt2_mdef.scala` is compiled by **real scalac** (only nsc writes the
//! `@macroImpl` annotation the type arguments travel in) and packed into a jar;
//! `mt2_use.scala` and `mt2_bad.scala` are then compiled by scala-rs against
//! it, and each of them also declares a macro def of its own so that the
//! source-side reader is exercised in the same run.
//!
//! The check that matters is the **dual run**: real scalac 2.13.16 compiles the
//! same files against each other and the two programs must print the same
//! thing. Every line of `mt2_use.scala` is a type argument that had to be
//! resolved rather than lined up -- `Plain.swapped[Int, String]` alone would
//! print `R=Int U=String` under the old rule and prints `R=String U=Int` under
//! nsc -- so a wrong resolution still compiles, still runs, and shows up only
//! here.
//!
//! The library is a **jar** rather than a directory on purpose: a class file
//! directory on `-cp` goes through `classpath::install_classpath`, which reads
//! its own pickle subset eagerly and installs a macro def as an ordinary method
//! with no binding at all. That is a separate defect (`docs/macros.md` §7.22,
//! "A defect this slice found and did not fix"); jars are what slick and
//! gitbucket actually are, and what the macro path is built for.

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
        "scala-rs-mapto2-{tag}-{}-{nanos}-{seq}",
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
    if !tool_available("java") || !tool_available("javac") || !tool_available("jar") {
        eprintln!("skip {tag}: java / javac / jar not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() || find_scalac().is_none() {
        eprintln!("skip {tag}: scala-library / scala-reflect / scalac not obtainable");
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

/// Compile `mt2_mdef.scala` with real scalac and pack the result into a jar.
///
/// Real scalac because only nsc writes the `MACRO` flag and the `@macroImpl`
/// annotation whose `TypeApply` carries the reference's type arguments; a jar
/// because that is the shape the pickle reader is reached through (see the
/// module comment).
fn build_library(tag: &str) -> PathBuf {
    let scalac = find_scalac().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let classes = tmp_dir(tag);
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            classes.to_str().unwrap(),
            fixtures_dir().join("mt2_mdef.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected mt2_mdef.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let jar = classes.join("mt2-lib.jar");
    let out = Command::new("jar")
        .arg("cf")
        .arg(&jar)
        .arg("-C")
        .arg(&classes)
        .arg("mt2")
        .output()
        .expect("jar");
    assert!(
        out.status.success(),
        "jar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    jar
}

/// Compile the named fixture with scala-rs against scala-reflect plus `lib`.
fn compile(name: &str, out: &Path, lib: &Path) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library");
    let reflect = scala_reflect_jar().expect("scala-reflect");
    Command::new(bin())
        .arg("compile")
        .arg(fixtures_dir().join(format!("{name}.scala")))
        .args([
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &format!("{}:{}", reflect.display(), lib.display()),
            "--scala-library",
        ])
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

fn classpath(uses: &Path, lib: &Path) -> String {
    format!(
        "{}:{}:{}:{}",
        uses.display(),
        lib.display(),
        scala_reflect_jar().unwrap().display(),
        scala_library_jar().unwrap().display()
    )
}

/// Seven implementation references, expanded for real and **executed**.
///
/// Six come out of the jar's pickle and one is a macro def this very run
/// compiles, so both readers of `MacroImplBinding.targs` are covered. The
/// expected output is real scalac's, recorded in
/// `tests/fixtures/expected/mt2_use.txt` and re-checked against scalac itself
/// by `mt2_reference_targs_match_real_scalac`.
#[test]
fn mt2_reference_targs_expand_and_run() {
    if !prerequisites("mt2_use") {
        return;
    }
    let lib = build_library("lib");
    let uses = tmp_dir("use");
    let out = compile("mt2_use", &uses, &lib);
    assert!(
        out.status.success(),
        "compile mt2_use failed: {}",
        diagnostics(&out)
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected/mt2_use.txt")).unwrap();
    assert_eq!(
        run_main(&classpath(&uses, &lib), "mt2_use"),
        expected,
        "stdout mismatch"
    );
    let _ = fs::remove_dir_all(&uses);
}

/// What real scalac 2.13.16 makes of the same two files: byte for byte the
/// same, with no exceptions carved out.
#[test]
fn mt2_reference_targs_match_real_scalac() {
    if !prerequisites("mt2_use scalac diff") {
        return;
    }
    let scalac = find_scalac().unwrap();
    let lib = build_library("lib-scalac");
    let uses = tmp_dir("use-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!(
                "{}:{}",
                scala_reflect_jar().unwrap().display(),
                lib.display()
            ),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("mt2_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected mt2_use.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ours = fs::read_to_string(fixtures_dir().join("expected/mt2_use.txt")).unwrap();
    assert_eq!(
        run_main(&classpath(&uses, &lib), "mt2_use (real scalac build)"),
        ours,
        "recorded expectation for mt2_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&uses);
}

/// Every reference type argument scala-rs will not resolve is refused **by
/// name**, through both readers.
///
/// This is the half that keeps the other half honest. All three call sites are
/// a program real scalac 2.13.16 compiles and runs, printing
///
/// ```text
/// R=List[A] U=Int
/// R=List[A] U=Int
/// R=Boolean U=U
/// ```
///
/// -- and neither answer is a reading of what the source wrote: `List[A]` is
/// `List`'s own type parameter and `U` is a free one. scala-rs accepts none of
/// the three rather than substituting `List[String]` and `Int`, which would
/// compile, run, and print something nsc never prints.
#[test]
fn mt2_unresolvable_reference_targs_are_named() {
    if !prerequisites("mt2_bad") {
        return;
    }
    let lib = build_library("bad-lib");
    let uses = tmp_dir("bad-use");
    let out = compile("mt2_bad", &uses, &lib);
    let text = diagnostics(&out);
    for want in [
        // An applied type constructor, out of the jar's pickle.
        "the implementation reference writes `List[R]` as a type argument",
        // The macro def's owner's type parameter with no receiver to read it
        // off.
        "a type parameter of `LocalInner`, which nsc reads off the receiver \
         the macro was called on; this call has no receiver",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(!out.status.success() || text.contains("error:"), "{text}");
    assert_eq!(
        text.matches("macro expansion is not implemented").count(),
        3,
        "every call site must be refused:\n{text}"
    );
    // Both readers say the same thing about `List[R]`: one reference is in the
    // jar's pickle and one is compiled in this run.
    assert_eq!(
        text.matches("the implementation reference writes `List[R]`")
            .count(),
        2,
        "both readers of the implementation reference must refuse it:\n{text}"
    );
    let _ = fs::remove_dir_all(&uses);
}
