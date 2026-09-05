//! E2E tests for miscellaneous scala/scala library syntax and semantics:
//! multi-importer imports, `using` clauses, `@specialized`/`@unspecialized`,
//! stack map frames at offset 0, case class companion `unapply`, trait
//! outer accessors, locally nested templates, stable identifier patterns and
//! `lub(Unit, A)`. Split out of `e2e.rs`.

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
    // Two tests can share a tag, and the clock is not fine enough to
    // separate them: they ran in the same directory and each `java Main` saw
    // the other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-e2e-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    assert!(
        out.join("Main$.class").is_file(),
        "Main$.class missing in {}",
        out.display()
    );
    out
}

fn compile_fixture(name: &str) -> PathBuf {
    // Private-runtime fixtures must not auto-link a discovered scala-library jar.
    compile_fixture_with(name, &["--no-scala-library"])
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_java(out: &Path) -> String {
    let output = Command::new("java")
        .args(["-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn check(name: &str) {
    let out = compile_fixture(name);
    if java_available() {
        let got = run_java(&out);
        let exp = expected_stdout(name);
        assert_eq!(got, exp, "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

const LIBRARY_COLLIDERS: &[&str] = &[
    "scala/Option.class",
    "scala/Some.class",
    "scala/Some$.class",
    "scala/None$.class",
    "scala/Function0.class",
    "scala/Function1.class",
    "scala/PartialFunction.class",
    "scala/Tuple2.class",
    "scala/NotImplementedError.class",
    "scala/collection/immutable/List.class",
    "scala/collection/immutable/$colon$colon.class",
    "scala/collection/immutable/Nil$.class",
    "scala/collection/immutable/List$.class",
    "scala/runtime/ArrowAssoc.class",
    "scala/Predef$.class",
    "scala/collection/StringOps.class",
    "scala/collection/ArrayOps.class",
    "scala/collection/ArrayOps$.class",
    "scala/collection/WithFilter.class",
    "scala/collection/Iterator.class",
    "scala/Option$WithFilter.class",
    "scala/collection/immutable/Map.class",
    "scala/collection/immutable/Map$.class",
    "scala/collection/immutable/Vector.class",
    "scala/collection/immutable/Vector$.class",
    "scala/collection/immutable/IndexedSeq.class",
    "scala/collection/immutable/IndexedSeq$.class",
    "scala/collection/immutable/Queue.class",
    "scala/collection/immutable/Queue$.class",
    "scala/Predef$any2stringadd.class",
    "scala/Predef$ArrowAssoc.class",
    "scala/runtime/RichInt.class",
    "scala/runtime/RichLong.class",
    "scala/runtime/RichDouble.class",
    "scala/runtime/RichChar.class",
    "scala/collection/immutable/Range.class",
    "scala/collection/immutable/Set.class",
    "scala/collection/immutable/Set$.class",
    "scala/collection/immutable/SortedSet.class",
    "scala/collection/immutable/SortedSet$.class",
    "scala/collection/immutable/TreeSet.class",
    "scala/collection/immutable/TreeSet$.class",
    "scala/collection/immutable/SortedMap.class",
    "scala/collection/immutable/SortedMap$.class",
    "scala/collection/immutable/TreeMap.class",
    "scala/collection/immutable/TreeMap$.class",
    "scala/collection/immutable/BitSet.class",
    "scala/collection/immutable/BitSet$.class",
    "scala/collection/immutable/Seq.class",
    "scala/collection/immutable/Seq$.class",
    "scala/collection/immutable/LazyList.class",
    "scala/collection/immutable/LazyList$.class",
    "scala/runtime/RichFloat.class",
    "scala/runtime/RichByte.class",
    "scala/runtime/RichShort.class",
    "scala/runtime/RichBoolean.class",
    "scala/collection/mutable/ArrayBuffer.class",
    "scala/collection/mutable/ArrayBuffer$.class",
    "scala/collection/mutable/ListBuffer.class",
    "scala/collection/mutable/ListBuffer$.class",
    "scala/collection/mutable/ArrayDeque.class",
    "scala/collection/mutable/ArrayDeque$.class",
    "scala/collection/mutable/StringBuilder.class",
    "scala/collection/mutable/StringBuilder$.class",
    "scala/collection/mutable/HashMap.class",
    "scala/collection/mutable/HashMap$.class",
    "scala/collection/mutable/HashSet.class",
    "scala/collection/mutable/HashSet$.class",
    "scala/collection/mutable/LinkedHashMap.class",
    "scala/collection/mutable/LinkedHashMap$.class",
    "scala/collection/mutable/LinkedHashSet.class",
    "scala/collection/mutable/LinkedHashSet$.class",
    "scala/collection/immutable/NumericRange.class",
    "scala/collection/immutable/NumericRange$.class",
    "scala/collection/immutable/NumericRange$Inclusive.class",
    "scala/collection/immutable/NumericRange$Exclusive.class",
    "scala/util/Either.class",
    "scala/util/Left.class",
    "scala/util/Right.class",
    "scala/App.class",
    "scala/DelayedInit.class",
    "scala/util/Left$.class",
    "scala/util/Right$.class",
    "scala/util/Try.class",
    "scala/util/Try$.class",
    "scala/util/Success.class",
    "scala/util/Success$.class",
    "scala/util/Failure.class",
    "scala/util/Failure$.class",
    "scala/util/control/Breaks.class",
    "scala/util/control/Breaks$.class",
    "scala/util/control/Breaks$TryBlock.class",
    "scala/util/control/Breaks$$anon$1.class",
    "scala/math/BigInt.class",
    "scala/math/BigInt$.class",
    "scala/math/BigDecimal.class",
    "scala/math/BigDecimal$.class",
    "scala/util/ChainingOps.class",
    "scala/util/ChainingOps$.class",
    "scala/util/package$chaining$.class",
    "scala/collection/View.class",
    "scala/collection/View$.class",
    "scala/collection/SeqView.class",
    "scala/util/Using.class",
    "scala/util/Using$.class",
    "scala/util/Using$Manager.class",
    "scala/util/Using$Manager$.class",
    "scala/util/Using$Releasable.class",
    "scala/util/Using$Releasable$.class",
    "scala/util/Using$Releasable$AutoCloseableIsReleasable$.class",
    "scala/util/ChainingSyntax.class",
    "scala/runtime/IntRef.class",
    "scala/runtime/ObjectRef.class",
    "scala/runtime/LongRef.class",
    "scala/runtime/BooleanRef.class",
    "scala/util/matching/Regex.class",
    "scala/Array$.class",
    "scala/runtime/NonLocalReturnControl.class",
];

fn assert_no_private_stdlib(out: &Path) {
    for rel in LIBRARY_COLLIDERS {
        let p = out.join(rel);
        assert!(
            !p.is_file(),
            "library ABI must not emit {} (would collide with scala-library.jar)",
            p.display()
        );
    }
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn scala_xml_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-xml_2.13-2.3.0.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/modules/scala-xml_2.13/2.3.0/scala-xml_2.13-2.3.0.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_no_private_stdlib(&out);
    let mut cp = format!("{}:{}", out.display(), jar.display());
    if let Some(xml) = scala_xml_jar() {
        cp.push(':');
        cp.push_str(&xml.display().to_string());
    }
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Syntax the 2.13 standard library uses that this subset did not parse:
/// several importers under one `import`, a `using` argument clause, an
/// annotation whose type is parenthesised and meta-annotated, and an
/// interpolation hole holding more than one statement (with a nested
/// triple-quoted string in it). See docs/scala-library.md.
#[test]
fn scalalib_syntax_fixture() {
    check("scalalib_syntax");
    dual_run_fixture("scalalib_syntax");
}

/// `@specialized` used to be a diagnostic without `-no-specialization`. It is
/// now accepted and recorded on the type parameter's symbol (stage 1 of
/// docs/specialization.md), so the same fixture compiles and runs with or
/// without the flag -- including the `@sp` spelling an import rename produces,
/// which this fixture also writes. What is still missing is the phase: no
/// `Cell$mcI$sp` is emitted, and `tests/spec_classfiles.sh` measures that gap
/// against real scalac.
///
/// (`scalalib_spec_bad.scala` existed only to assert the diagnostic for the
/// renamed spelling. There is no diagnostic to assert any more, and the alias
/// is covered here and by `sp_alias`, so the fixture is gone.)
#[test]
fn scalalib_specialized_is_accepted_without_the_flag() {
    dual_run_fixture("scalalib_spec");
}

/// With nsc's `-no-specialization` the annotations are ignored, as nsc ignores
/// them. Needs the jar: `scala.specialized` is not in the private runtime.
#[test]
fn scalalib_specialized_ignored_with_the_flag() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scalalib_spec: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(
        "scalalib_spec",
        &["--scala-library", jar_s, "-no-specialization"],
    );
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "Main"])
            .output()
            .expect("java");
        assert!(
            output.status.success(),
            "java failed for scalalib_spec: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected_stdout("scalalib_spec")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Stack map frames at bytecode offset 0, `athrow` on a `Nothing`-typed tree
/// whose generated value is not a `Throwable`, and the cast a non-local return
/// needs before `areturn`. All three were rejected by the JVM verifier, so the
/// fixture is run in both library modes (here and in the dual run below).
#[test]
fn fixtures_rv_verifyframes() {
    check("rv_verifyframes");
}

#[test]
fn scala_library_dual_run_rv_verifyframes() {
    dual_run_fixture("rv_verifyframes");
}

/// A case class's companion `unapply` has to be a real method: the pattern
/// matcher never calls it, but user code does. Library mode only -- the
/// expected output prints `Option` and `TupleN`, which the private runtime
/// renders differently and does not have past `Tuple2`.
#[test]
fn scala_library_dual_run_rv_caseunapply() {
    dual_run_fixture("rv_caseunapply");
}

/// A trait's own superclass becomes the implementing template's superclass
/// (SLS 5.1.2), and a trait nested in a trait or class gets its
/// `<Trait>$$$outer()` implemented from the enclosing object.
#[test]
fn fixtures_rv_traitouter() {
    check("rv_traitouter");
}

#[test]
fn scala_library_dual_run_rv_traitouter() {
    dual_run_fixture("rv_traitouter");
}

/// Templates declared inside a *local* class reach a classfile, and an
/// imported name that the object only inherits keeps its receiver.
#[test]
fn fixtures_rv_localnested() {
    check("rv_localnested");
}

#[test]
fn scala_library_dual_run_rv_localnested() {
    dual_run_fixture("rv_localnested");
}

/// Stable identifier patterns: a name that resolves to a `val`, a backquoted
/// name, both inside an alternative and inside a `for` generator, plus the
/// compound type pattern that has to test every parent. Each of these used to
/// bind a fresh variable (or test only the first parent), so the case matched
/// everything and the program answered from the wrong branch. The expected
/// output is real scalac 2.13.16's.
#[test]
fn stable_identifier_patterns() {
    dual_run_fixture("rm_stable_pattern");
}

/// A `case class` companion's `toString`, a `var` constructor parameter
/// assigned in the class body, and `lub(Unit, A)` for an abstract `A` -- three
/// wrong answers that no diagnostic reported.
#[test]
fn case_companion_var_param_and_unit_lub() {
    dual_run_fixture("rm_classbody");
}
