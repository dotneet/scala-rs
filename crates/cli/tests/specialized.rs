//! E2E tests for the method-owned `@specialized` slice. Class and trait
//! specialization remain outside this fixture file's first-slice coverage.

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

fn compile_fails(name: &str, needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
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

/// `@specialized` and `@unspecialized` are accepted and recorded on the
/// symbol; the `specialize` phase is not implemented, so nothing `$mc*$sp`
/// comes out. The program runs, and it computes what the same program without
/// the annotation computes -- which is the whole reason accepting it is sound
/// while the phase is missing. `tests/spec_classfiles.sh` is the ledger that
/// keeps the gap visible; see docs/specialization.md.
#[test]
fn sp_annot_fixture() {
    check("sp_annot");
    dual_run_fixture("sp_annot");
}

/// `import scala.{specialized => sp}` is how cats and the collections spell
/// it. Library mode only: the private runtime has no `scala.specialized` for
/// the import to name.
#[test]
fn sp_alias_fixture() {
    dual_run_fixture("sp_alias");
}

/// The annotation is a performance hint and must not soften type checking.
#[test]
fn sp_annot_bad_is_rejected() {
    compile_fails("sp_annot_bad", "type mismatch");
}

fn javap_class(out: &Path, class: &str, flags: &[&str]) -> String {
    let mut cmd = Command::new("javap");
    cmd.args(["-classpath", out.to_str().unwrap()]);
    cmd.args(flags);
    cmd.arg(class);
    let output = cmd.output().expect("javap");
    assert!(
        output.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The first slice must keep the generic method, emit only the supported
/// Int/Long entries, and leave both unsupported Double selections and
/// override-capable class methods on the generic path. The body deliberately
/// exercises a local, nested def, captured closure, early return, and
/// recursion so the primitive clone cannot share stale generic symbols.
#[test]
fn method_specialization_abi_and_body_fixture() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip method specialization ABI fixture: scala-library unavailable");
        return;
    };
    let root = tmp_dir("method-specialization-abi");
    let src = root.join("MethodAbi.scala");
    fs::write(
        &src,
        r#"import scala.specialized

object MixedOps {
  def id[@specialized(Int, Long, Double) A](x: A): A = {
    val y: A = x
    def nested(z: A): A = z
    val f: A => A = (v: A) => nested(v)
    return f(y)
  }

  def recurse[@specialized(Int) A](x: A): A = {
    def loop(n: Int, z: A): A = if (n == 0) z else loop(n - 1, z)
    loop(2, x)
  }
}

class NonFinalHost {
  def id[@specialized(Int, Long) A](x: A): A = x
}

object MethodAbiMain {
  def main(args: Array[String]): Unit = {
    if (MixedOps.id(7) != 7) throw new RuntimeException("int")
    if (MixedOps.id(7L) != 7L) throw new RuntimeException("long")
    if (MixedOps.recurse(9) != 9) throw new RuntimeException("recursive")
    println("method-specialization-ok")
  }
}
"#,
    )
    .unwrap();
    let out = root.join("provider");
    fs::create_dir_all(&out).unwrap();
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs method specialization compile");
    assert!(status.success(), "method specialization provider failed");

    let module = javap_class(&out, "MixedOps$", &["-p", "-s"]);
    assert!(
        module.contains("id$mIc$sp"),
        "missing Int variant: {module}"
    );
    assert!(
        module.contains("id$mJc$sp"),
        "missing Long variant: {module}"
    );
    assert!(
        module.contains("recurse$mIc$sp"),
        "missing recursive variant: {module}"
    );
    assert!(
        module.contains("<A> A id(A)"),
        "missing generic fallback: {module}"
    );
    assert!(
        !module.contains("id$mDc$sp"),
        "unsupported Double variant was emitted: {module}"
    );
    let non_final = javap_class(&out, "NonFinalHost", &["-p", "-s"]);
    assert!(
        !non_final.contains("id$mIc$sp") && !non_final.contains("id$mJc$sp"),
        "override-capable method was specialized: {non_final}"
    );

    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "MethodAbiMain",
        ])
        .output()
        .expect("java method specialization fixture");
    assert!(
        run.status.success(),
        "method specialization runtime failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "method-specialization-ok\n"
    );

    let scalac = Path::new("/tmp/scala-2.13.16/bin/scalac");
    if scalac.is_file() {
        let consumer_src = root.join("MethodAbiConsumer.scala");
        fs::write(
            &consumer_src,
            r#"object MethodAbiConsumer {
  def i: Int = MixedOps.id(7)
  def j: Long = MixedOps.id(7L)
  def d: Double = MixedOps.id(1.0)
  def r: Int = MixedOps.recurse(9)
}
"#,
        )
        .unwrap();
        let consumer_out = root.join("consumer");
        fs::create_dir_all(&consumer_out).unwrap();
        let cp = format!("{}:{}", out.display(), jar.display());
        let status = Command::new(scalac)
            .args([
                "-cp",
                &cp,
                "-d",
                consumer_out.to_str().unwrap(),
                consumer_src.to_str().unwrap(),
            ])
            .status()
            .expect("run scalac method specialization consumer");
        assert!(status.success(), "scalac consumer failed");
        let consumer = javap_class(&consumer_out, "MethodAbiConsumer$", &["-c", "-p"]);
        assert!(
            consumer.contains("MixedOps$.id$mIc$sp:(I)I"),
            "scalac did not select Int variant: {consumer}"
        );
        assert!(
            consumer.contains("MixedOps$.id$mJc$sp:(J)J"),
            "scalac did not select Long variant: {consumer}"
        );
        assert!(
            consumer.contains("MixedOps$.id:(Ljava/lang/Object;)Ljava/lang/Object;"),
            "scalac did not retain generic Double fallback: {consumer}"
        );
        assert!(
            !consumer.contains("id$mDc$sp"),
            "scalac selected an unadvertised Double variant: {consumer}"
        );
    } else {
        eprintln!("skip scalac separate consumer: /tmp/scala-2.13.16/bin/scalac unavailable");
    }
    let _ = fs::remove_dir_all(&root);
}
