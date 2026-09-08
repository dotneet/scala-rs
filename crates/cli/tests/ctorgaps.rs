//! `agent/ctorgaps`: constructor privacy across the class-file round trip.
//!
//! `agent/intrinsicqual` implemented the access check for `private` and
//! `protected` constructors and closed four corpus `neg` tests, and left one
//! open:
//!
//! > `neg/t6601` is NOT closed: it is a separate compilation, and constructor
//! > privacy does not survive the class-file round trip.
//!
//! It cannot survive it through the *class file*. nsc emits even a `private`
//! constructor `ACC_PUBLIC` -- `javap -p` on
//! `class PrivateConstructor private(val s: String) extends AnyVal`, compiled
//! by scalac 2.13.16, says `public PrivateConstructor(java.lang.String)`. The
//! only record is the `ScalaSignature`, and `PickleSupply::supply_ctors` was
//! filtering constructors through `Member::is_public_api`, which **hides** a
//! private member outright: the private `<init>` was dropped, the class file's
//! `ACC_PUBLIC` one stayed, and `new PrivateConstructor("")` compiled.
//!
//! So the constructor is now supplied *and marked* rather than dropped, and
//! the check `agent/intrinsicqual` wrote fires on it.
//!
//! Every test here is a **separate compilation** -- one `scala-rs` run writes
//! class files, a second reads them back -- because that is the only shape in
//! which the defect exists. Where it matters, real scalac 2.13.16 reads the
//! same class files, so "our reader agrees with our writer" cannot pass for
//! agreement with nsc.
//!
//! Fixture prefix: `ctorgaps_`.

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
        "scala-rs-ctorgaps-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Real scalac 2.13.16, when this machine has the checkout the measurement
/// scripts install. Every test that needs it skips loudly without it.
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

/// Write `src` to `dir/name.scala` and return its path.
fn write_src(dir: &Path, name: &str, src: &str) -> PathBuf {
    let p = dir.join(format!("{name}.scala"));
    fs::write(&p, src).unwrap();
    p
}

struct Run {
    ok: bool,
    text: String,
}

/// scala-rs `compile`, against the jar, with `cp` on the classpath.
fn compile_rs(src: &Path, out: &Path, jar: &Path, cp: Option<&Path>) -> Run {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    if let Some(cp) = cp {
        cmd.args(["-classpath", cp.to_str().unwrap()]);
    }
    let output = cmd.output().expect("run scala-rs compile");
    Run {
        ok: output.status.success(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// Real scalac, same shape.
fn compile_scalac(scalac: &Path, src: &Path, out: &Path, jar: &Path, cp: Option<&Path>) -> Run {
    let mut classpath = jar.display().to_string();
    if let Some(cp) = cp {
        classpath.push(':');
        classpath.push_str(&cp.display().to_string());
    }
    let output = Command::new(scalac)
        .args([
            "-classpath",
            &classpath,
            "-d",
            out.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .expect("run scalac");
    Run {
        ok: output.status.success(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

fn rejected(run: Run, what: &str, needle: &str) {
    assert!(
        !run.ok && run.text.contains(needle),
        "expected {what} to be rejected with {needle:?}; got ok={} :\n{}",
        run.ok,
        run.text
    );
}

/// scala/scala's own `test/files/neg/t6601`, verbatim, plus its `.check`.
///
/// ```scala
/// // PrivateConstructor_1.scala
/// class PrivateConstructor private(val s: String) extends AnyVal
/// // AccessPrivateConstructor_2.scala
/// class AccessPrivateConstructor {
///   new PrivateConstructor("")
/// }
/// ```
///
/// The `.check` is one line, and this branch reproduces it word for word,
/// reading a class file **this compiler wrote**. An unmodified build of the
/// branch point compiles the second half without a diagnostic.
const T6601_LIB: &str = "class PrivateConstructor private(val s: String) extends AnyVal\n";
const T6601_USE: &str = "class AccessPrivateConstructor {\n  new PrivateConstructor(\"\")\n}\n";
/// `neg/t6601.check`, minus partest's `<file>:<line>: error: ` prefix.
const T6601_CHECK: &str = "constructor PrivateConstructor in class PrivateConstructor cannot \
     be accessed in class AccessPrivateConstructor from class AccessPrivateConstructor";

#[test]
fn t6601_our_class_file_read_back_by_us() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip t6601: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("t6601");
    let lib_src = write_src(&dir, "PrivateConstructor_1", T6601_LIB);
    let use_src = write_src(&dir, "AccessPrivateConstructor_2", T6601_USE);
    let lib = dir.join("lib");
    let app = dir.join("app");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&app).unwrap();
    ok(compile_rs(&lib_src, &lib, &jar, None), "t6601 library half");
    rejected(
        compile_rs(&use_src, &app, &jar, Some(&lib)),
        "t6601 caller against our own class file",
        T6601_CHECK,
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same, reading **real scalac's** class file. Our writer and our reader
/// agreeing is not evidence that either matches nsc; this is the half that
/// says the pickle we read is the one nsc writes.
#[test]
fn t6601_scalac_class_file_read_back_by_us() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip t6601 (scalac): jar or scalac not present");
        return;
    };
    let dir = tmp_dir("t6601sc");
    let lib_src = write_src(&dir, "PrivateConstructor_1", T6601_LIB);
    let use_src = write_src(&dir, "AccessPrivateConstructor_2", T6601_USE);
    let lib = dir.join("lib");
    let app = dir.join("app");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&app).unwrap();
    ok(
        compile_scalac(&sc, &lib_src, &lib, &jar, None),
        "t6601 library half under scalac",
    );
    // The premise: nsc's own class file says `public`. If this ever stops
    // holding, the test below is proving something else.
    let javap = Command::new("javap")
        .args(["-p", "-cp", lib.to_str().unwrap(), "PrivateConstructor"])
        .output()
        .expect("javap");
    let text = String::from_utf8_lossy(&javap.stdout);
    assert!(
        text.contains("public PrivateConstructor(java.lang.String)"),
        "nsc is expected to emit the private constructor ACC_PUBLIC; got:\n{text}"
    );
    rejected(
        compile_rs(&use_src, &app, &jar, Some(&lib)),
        "t6601 caller against scalac's class file",
        T6601_CHECK,
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The whole access ladder over one separately compiled library, in both
/// directions: what must still compile, and what must now be refused.
///
/// Written as one library and several callers because the risk of this change
/// is **over**-rejection -- hiding a constructor that is genuinely callable
/// breaks separate compilation with no test to catch it. Every accepted case
/// here is one an unmodified build also accepted.
const LADDER_LIB: &str = r#"
package libp

class Priv private (val s: String) { def show: String = "priv:" + s }
object Priv { def make(s: String): Priv = new Priv(s) }

class Qual private[libp] (val s: String) { def show: String = "qual:" + s }
object Qual { def make(s: String): Qual = new Qual(s) }

class Prot protected (val s: String) { def show: String = "prot:" + s }
object Prot { def make(s: String): Prot = new Prot(s) }

class Mixed(val v: String) {
  private def this(len: Int) = this("len" + len)
  def show: String = "mixed:" + v
}

class Pub(val s: String) { def show: String = "pub:" + s }
"#;

/// Everything a separate compilation may legally do with those constructors.
/// It also *runs*, so a constructor that resolved to the wrong alternative
/// prints the wrong string rather than passing quietly.
const LADDER_OK: &str = r#"
package libp

class SubProt extends Prot("sub")
object Main {
  def main(a: Array[String]): Unit = {
    println(Priv.make("a").show)
    println(new Qual("b").show)
    println(Qual.make("c").show)
    println(Prot.make("d").show)
    println(new SubProt().show)
    println(new Mixed("e").show)
    println(new Pub("f").show)
  }
}
"#;

const LADDER_EXPECTED: &str = "priv:a\nqual:b\nqual:c\nprot:d\nprot:sub\nmixed:e\npub:f\n";

fn ladder_lib(dir: &Path, jar: &Path) -> PathBuf {
    let src = write_src(dir, "LadderLib", LADDER_LIB);
    let lib = dir.join("lib");
    fs::create_dir_all(&lib).unwrap();
    ok(compile_rs(&src, &lib, jar, None), "the ladder library");
    lib
}

#[test]
fn accessible_constructors_still_compile_and_run_across_the_round_trip() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip ladder: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("ladderok");
    let lib = ladder_lib(&dir, &jar);
    let src = write_src(&dir, "LadderOk", LADDER_OK);
    let app = dir.join("app");
    fs::create_dir_all(&app).unwrap();
    ok(
        compile_rs(&src, &app, &jar, Some(&lib)),
        "the accessible half of the ladder",
    );
    if java_available() {
        let cp = format!("{}:{}:{}", app.display(), lib.display(), jar.display());
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "libp.Main"])
            .output()
            .expect("java");
        assert!(
            output.status.success(),
            "java libp.Main failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            LADDER_EXPECTED,
            "the ladder resolved a constructor to the wrong alternative"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// `private` and `protected` from outside, against a class file we wrote.
/// Both were accepted before this slice; scalac 2.13.16 refuses both with the
/// sentence asserted here (it adds an indented explanation for the
/// `protected` case, which this compiler does not print for any `protected`
/// access -- see `docs/comparison-with-scalac.md`).
#[test]
fn inaccessible_constructors_are_refused_across_the_round_trip() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip ladder (neg): scala-library jar not present");
        return;
    };
    let dir = tmp_dir("ladderbad");
    let lib = ladder_lib(&dir, &jar);
    let app = dir.join("app");
    fs::create_dir_all(&app).unwrap();
    for (name, src, needle) in [
        (
            "BadPriv",
            "package other\nclass BadPriv { new libp.Priv(\"x\") }\n",
            "constructor Priv in class Priv cannot be accessed in class BadPriv \
             from class BadPriv in package other",
        ),
        (
            "BadProt",
            "package other\nclass BadProt { new libp.Prot(\"x\") }\n",
            "constructor Prot in class Prot cannot be accessed in class BadProt \
             from class BadProt in package other",
        ),
    ] {
        let s = write_src(&dir, name, src);
        rejected(compile_rs(&s, &app, &jar, Some(&lib)), name, needle);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// nsc removes an inaccessible alternative *before* overload resolution, so
/// `new Mixed(5)` outside the class is a type error against the surviving
/// `String` constructor and not an access error. Reading the private
/// secondary back out of the pickle must not change that -- it is now an
/// alternative where before it was dropped entirely.
#[test]
fn an_inaccessible_alternative_does_not_become_the_pick() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip mixed: jar or scalac not present");
        return;
    };
    let dir = tmp_dir("laddermixed");
    let lib = ladder_lib(&dir, &jar);
    let app = dir.join("app");
    let ref_out = dir.join("ref");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&ref_out).unwrap();
    let src = write_src(
        &dir,
        "BadMixed",
        "package other\nclass BadMixed { new libp.Mixed(5) }\n",
    );
    rejected(
        compile_rs(&src, &app, &jar, Some(&lib)),
        "new Mixed(5) from outside",
        "required: String",
    );
    // And it is a type error for scalac too, against its own library half.
    let sc_lib = dir.join("sclib");
    fs::create_dir_all(&sc_lib).unwrap();
    let lib_src = dir.join("LadderLib.scala");
    ok(
        compile_scalac(&sc, &lib_src, &sc_lib, &jar, None),
        "the ladder library under scalac",
    );
    let run = compile_scalac(&sc, &src, &ref_out, &jar, Some(&sc_lib));
    assert!(
        !run.ok && run.text.contains("required: String"),
        "scalac is expected to report a type error here; got:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Found here, not fixed here: `private[p]` / `protected[p]` on a constructor
/// does not survive the round trip, and is left **accessible** on purpose.
///
/// nsc pickles a qualified access as the bare `PRIVATE` flag plus a
/// `privateWithin` reference; this reader records that the reference is there
/// (`Member::private_within`) but does not resolve `p`. Marking such a
/// constructor `private` on the strength of the flag alone would refuse every
/// `private[slick]` constructor slick's own code calls -- an over-rejection,
/// which is the one failure mode this change must not have. So it keeps the
/// accessibility it had before the slice, and scalac refuses one program we
/// accept.
///
/// The assertion is on the acceptance, so this test fails and says so when a
/// later slice resolves the boundary.
#[test]
fn a_qualified_private_constructor_is_still_accepted_from_outside() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip qualified private: jar or scalac not present");
        return;
    };
    let dir = tmp_dir("ladderqual");
    let lib = ladder_lib(&dir, &jar);
    let app = dir.join("app");
    let ref_out = dir.join("ref");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&ref_out).unwrap();
    let src = write_src(
        &dir,
        "BadQual",
        "package other\nclass BadQual { new libp.Qual(\"x\") }\n",
    );
    ok(
        compile_rs(&src, &app, &jar, Some(&lib)),
        "the known `private[p]` gap",
    );
    let sc_lib = dir.join("sclib");
    fs::create_dir_all(&sc_lib).unwrap();
    let lib_src = dir.join("LadderLib.scala");
    ok(
        compile_scalac(&sc, &lib_src, &sc_lib, &jar, None),
        "the ladder library under scalac",
    );
    let run = compile_scalac(&sc, &src, &ref_out, &jar, Some(&sc_lib));
    assert!(
        !run.ok && run.text.contains("cannot be accessed"),
        "scalac is expected to refuse `private[libp]` from outside; got:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The other half of the slice, across the same seam: a **secondary**
/// constructor's default argument, whose `$lessinit$greater$default$n` is
/// written to the companion by `crate::ctor_defaults` and read back by
/// `default_getter_apply`.
///
/// Real scalac reads the same class file and fills the same default, which is
/// what says the getter is nsc's and not merely ours. An unmodified build of
/// the branch point writes a class file scalac rejects: `not enough arguments
/// for constructor SecDef: (n: Int, sep: String): SecDef`.
#[test]
fn a_secondary_ctor_default_survives_the_round_trip() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip secondary default: scala-library jar not present");
        return;
    };
    let lib_src = "class SecDef(val label: String) {\n  \
                   def this(n: Int, sep: String = \"-\") = this(sep + n + sep)\n}\nobject SecDef\n";
    let use_src = "object Main {\n  def main(a: Array[String]): Unit = {\n    \
                   println(new SecDef(4).label)\n    \
                   println(new SecDef(4, \"*\").label)\n  }\n}\n";
    let dir = tmp_dir("secdefsplit");
    let lib = dir.join("lib");
    fs::create_dir_all(&lib).unwrap();
    let l = write_src(&dir, "SecDefLib", lib_src);
    ok(compile_rs(&l, &lib, &jar, None), "the SecDef library");
    let u = write_src(&dir, "SecDefUse", use_src);

    let app = dir.join("app");
    fs::create_dir_all(&app).unwrap();
    ok(
        compile_rs(&u, &app, &jar, Some(&lib)),
        "the SecDef caller under scala-rs",
    );
    if java_available() {
        let cp = format!("{}:{}:{}", app.display(), lib.display(), jar.display());
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "Main"])
            .output()
            .expect("java");
        assert!(
            output.status.success(),
            "java Main failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "-4-\n*4*\n");
    }

    let Some(sc) = scalac() else {
        eprintln!("skip the scalac half: scalac not present");
        return;
    };
    let sc_app = dir.join("scapp");
    fs::create_dir_all(&sc_app).unwrap();
    ok(
        compile_scalac(&sc, &u, &sc_app, &jar, Some(&lib)),
        "real scalac reading our SecDef class file",
    );
    if java_available() {
        let cp = format!("{}:{}:{}", sc_app.display(), lib.display(), jar.display());
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "Main"])
            .output()
            .expect("java");
        assert!(
            output.status.success(),
            "java Main (scalac-compiled caller) failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "-4-\n*4*\n");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The fixtures, so `tests/fixtures/ctorgaps_*.scala` are not orphaned when
/// `e2e.rs` is filtered out: the positive one exists and the compiler agrees
/// with the recorded expected output in library mode.
#[test]
fn the_secondary_default_fixture_matches_its_expected_output() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip fixture check: jar or java not present");
        return;
    };
    let out = tmp_dir("fixture");
    let src = fixtures_dir().join("ctorgaps_secdefault.scala");
    ok(compile_rs(&src, &out, &jar, None), "the fixture");
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let exp = fs::read_to_string(fixtures_dir().join("expected/ctorgaps_secdefault.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), exp);
    let _ = fs::remove_dir_all(&out);
}
