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

  def poly[@specialized(Int, Long) A](a: A, n: Int): String =
    if (n == 0) a.toString else poly[String]("ok", 0)

  def unused[@specialized(Int, Long) A](x: Int): Int = x
}

class NonFinalHost {
  def id[@specialized(Int, Long) A](x: A): A = x
}

object MethodAbiMain {
  def main(args: Array[String]): Unit = {
    if (MixedOps.id(7) != 7) throw new RuntimeException("int")
    if (MixedOps.id(7L) != 7L) throw new RuntimeException("long")
    if (MixedOps.recurse(9) != 9) throw new RuntimeException("recursive")
    if (MixedOps.poly(1, 1) != "ok") throw new RuntimeException("poly-int")
    if (MixedOps.poly(2L, 1) != "ok") throw new RuntimeException("poly-long")
    if (MixedOps.unused(3) != 3) throw new RuntimeException("unused")
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
        module.contains("poly$mIc$sp") && module.contains("poly$mJc$sp"),
        "missing polymorphic-recursion variants: {module}"
    );
    assert!(
        !module.contains("unused$mIc$sp") && !module.contains("unused$mJc$sp"),
        "unused type parameter unexpectedly specialized: {module}"
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
  def p: String = MixedOps.poly(1, 1)
  def q: String = MixedOps.poly(2L, 1)
  def u: Int = MixedOps.unused(3)
  def ui: Int = MixedOps.unused[Int](4)
  def ul: Int = MixedOps.unused[Long](5)

  def main(args: Array[String]): Unit = {
    if (i != 7 || j != 7L || d != 1.0 || r != 9 || p != "ok" || q != "ok" || u != 3 || ui != 4 || ul != 5)
      throw new RuntimeException(s"$i:$j:$d:$r:$p:$q:$u:$ui:$ul")
    println(s"$i:$j:$d:$r:$p:$q:$u:$ui:$ul")
  }
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
        assert!(
            consumer.contains("MixedOps$.unused:(I)I")
                && !consumer.contains("unused$mIc$sp")
                && !consumer.contains("unused$mJc$sp"),
            "scalac did not retain the unused-parameter generic fallback: {consumer}"
        );
        let consumer_run = Command::new("java")
            .args([
                "-Xverify:all",
                "-cp",
                &format!(
                    "{}:{}:{}",
                    consumer_out.display(),
                    out.display(),
                    jar.display()
                ),
                "MethodAbiConsumer",
            ])
            .output()
            .expect("execute scalac method specialization consumer");
        assert!(
            consumer_run.status.success(),
            "scalac consumer runtime failed: {}",
            String::from_utf8_lossy(&consumer_run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&consumer_run.stdout),
            "7:7:1.0:9:ok:ok:3:4:5\n"
        );
    } else {
        eprintln!("skip scalac separate consumer: /tmp/scala-2.13.16/bin/scalac unavailable");
    }
    let _ = fs::remove_dir_all(&root);
}

/// A local class belongs to each cloned method body. Reusing its generic JVM
/// name makes the primitive clone overwrite the generic class and leaves the
/// clone calling a constructor descriptor that no longer exists.
#[test]
fn method_specialization_local_class_fixture() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip local-class specialization fixture: scala-library unavailable");
        return;
    };
    let root = tmp_dir("method-specialization-local-class");
    let src = root.join("LocalClassAbi.scala");
    fs::write(
        &src,
        r#"import scala.specialized

object LocalClassAbi {
  def wrap[@specialized(Int, Long) A](a: A): A = {
    class C(val x: A)
    new C(a).x
  }
}

object LocalClassAbiMain {
  def main(args: Array[String]): Unit = {
    println(LocalClassAbi.wrap(7) + ":" + LocalClassAbi.wrap(8L))
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
        .expect("run scala-rs local-class specialization compile");
    assert!(
        status.success(),
        "local-class specialization provider failed"
    );
    for suffix in ["$1", "$2", "$3"] {
        assert!(
            out.join(format!("LocalClassAbi$C{suffix}.class")).is_file(),
            "missing local class variant {suffix}"
        );
    }
    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "LocalClassAbiMain",
        ])
        .output()
        .expect("java local-class specialization fixture");
    assert!(
        run.status.success(),
        "local-class specialization runtime failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7:8\n");
    let _ = fs::remove_dir_all(&root);
}

/// A nested class's JVM prefix includes the enclosing local-class index. Each
/// method variant therefore needs a matching `Outer$N$Inner` pair; keeping the
/// generic `Outer$1$Inner` prefix makes the primitive body call a constructor
/// in a different class file. The nested object case below is intentionally
/// ours-only: scalac 2.13.16 rejects a local object that mentions the method's
/// type parameter, while scala-rs already accepts the shape as a static
/// singleton and must keep its three generated module classes distinct.
#[test]
fn method_specialization_nested_local_types_fixture() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip nested local specialization fixture: scala-library unavailable");
        return;
    };
    let root = tmp_dir("method-specialization-nested-local-types");
    let src = root.join("NestedLocalTypes.scala");
    fs::write(
        &src,
        r#"import scala.specialized

object NestedAbi {
  def wrap[@specialized(Int, Long) A](a: A): A = {
    class Outer { class Inner(val x: A) }
    val o = new Outer
    new o.Inner(a).x
  }
}

object LocalObjectAbi {
  def wrap[@specialized(Int, Long) A](a: A): A = {
    object O { def cast(x: Any): A = x.asInstanceOf[A] }
    O.cast(a)
  }
}

object NestedLocalTypesMain {
  def main(args: Array[String]): Unit = {
    println(NestedAbi.wrap(7))
    println(NestedAbi.wrap(8L))
    println(NestedAbi.wrap("s"))
    println(LocalObjectAbi.wrap(9))
    println(LocalObjectAbi.wrap(10L))
    println(LocalObjectAbi.wrap("t"))
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
        .expect("run scala-rs nested local specialization compile");
    assert!(
        status.success(),
        "nested local specialization provider failed"
    );
    for suffix in ["$1", "$2", "$3"] {
        assert!(
            out.join(format!("NestedAbi$Outer{suffix}.class")).is_file(),
            "missing nested Outer variant {suffix}"
        );
        assert!(
            out.join(format!("NestedAbi$Outer{suffix}$Inner.class"))
                .is_file(),
            "missing nested Inner variant {suffix}"
        );
        assert!(
            out.join(format!("LocalObjectAbi$O{suffix}$.class"))
                .is_file(),
            "missing local object variant {suffix}"
        );
    }
    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "NestedLocalTypesMain",
        ])
        .output()
        .expect("java nested local specialization fixture");
    assert!(
        run.status.success(),
        "nested local specialization runtime failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n8\ns\n9\n10\nt\n");
    let _ = fs::remove_dir_all(&root);
}

/// Keep every type-tree edge reachable while cloning a method variant. The
/// local class combines a self type, a local type alias, and an annotated type
/// use; its primitive clones must retain the alias's substituted JVM method
/// descriptor instead of sharing the generic symbols.
#[test]
fn method_specialization_type_tree_traversal_fixture() {
    let jar = scala_library_jar().expect("type-tree traversal requires scala-library");
    let root = tmp_dir("method-specialization-type-tree-traversal");
    let src = root.join("TraversalAbi.scala");
    fs::write(
        &src,
        r#"import scala.specialized

object TraversalAbi {
  def run[@specialized(Int, Long) A](a: A): String = {
    class Local {
      self: Local =>
      type Alias = A
      type Higher[X] = (A, X)
      type Bounded >: Null <: Any
      def id(x: Alias @unchecked): Alias = x
    }
    val x = new Local
    x.id(a).toString
  }
}

object TraversalAbiMain {
  def main(args: Array[String]): Unit = {
    println(TraversalAbi.run(7))
    println(TraversalAbi.run(8L))
    println(TraversalAbi.run("s"))
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
        .expect("run scala-rs type-tree traversal provider");
    assert!(status.success(), "type-tree traversal provider failed");

    let generic = javap_class(&out, "TraversalAbi$Local$1", &["-p", "-s"]);
    assert!(
        generic.contains("java.lang.Object id(java.lang.Object)")
            && generic.contains("descriptor: (Ljava/lang/Object;)Ljava/lang/Object;"),
        "generic local type-tree entry changed: {generic}"
    );
    let int_variant = javap_class(&out, "TraversalAbi$Local$2", &["-p", "-s"]);
    assert!(
        int_variant.contains("int id(int)") && int_variant.contains("descriptor: (I)I"),
        "Int local type-tree entry lost primitive owner/type substitution: {int_variant}"
    );
    let long_variant = javap_class(&out, "TraversalAbi$Local$3", &["-p", "-s"]);
    assert!(
        long_variant.contains("long id(long)") && long_variant.contains("descriptor: (J)J"),
        "Long local type-tree entry lost primitive owner/type substitution: {long_variant}"
    );

    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "TraversalAbiMain",
        ])
        .output()
        .expect("run scala-rs type-tree traversal provider");
    assert!(
        run.status.success(),
        "type-tree traversal runtime failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n8\ns\n");

    let scalac = Path::new("/tmp/scala-2.13.16/bin/scalac");
    assert!(
        scalac.is_file(),
        "type-tree traversal requires scalac 2.13.16"
    );
    {
        let nsc = root.join("nsc");
        fs::create_dir_all(&nsc).unwrap();
        let status = Command::new(scalac)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                nsc.to_str().unwrap(),
                src.to_str().unwrap(),
            ])
            .status()
            .expect("run scalac type-tree traversal provider");
        assert!(
            status.success(),
            "scalac type-tree traversal provider failed"
        );
        for (class, shape) in [
            (
                "TraversalAbi$Local$1",
                "descriptor: (Ljava/lang/Object;)Ljava/lang/Object;",
            ),
            ("TraversalAbi$Local$2", "descriptor: (I)I"),
            ("TraversalAbi$Local$3", "descriptor: (J)J"),
        ] {
            let nsc_class = javap_class(&nsc, class, &["-p", "-s"]);
            assert!(
                nsc_class.contains(shape),
                "scalac type-tree ABI changed for {class}: {nsc_class}"
            );
        }
        let nsc_run = Command::new("java")
            .args([
                "-Xverify:all",
                "-cp",
                &format!("{}:{}", nsc.display(), jar.display()),
                "TraversalAbiMain",
            ])
            .output()
            .expect("run scalac type-tree traversal provider");
        assert!(
            nsc_run.status.success(),
            "scalac type-tree traversal runtime failed: {}",
            String::from_utf8_lossy(&nsc_run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&nsc_run.stdout), "7\n8\ns\n");
    }
    let _ = fs::remove_dir_all(&root);
}

/// Constructor references inside a variant must be split by ownership. Local
/// classes and their constructors are cloned, while `String` and a source
/// class remain external references. The captured local class also checks
/// that its field and method symbols move to the primitive clone together.
#[test]
fn method_specialization_constructor_and_capture_fixture() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip constructor specialization fixture: scala-library unavailable");
        return;
    };
    let root = tmp_dir("method-specialization-constructor");
    let src = root.join("ConstructorReview.scala");
    fs::write(
        &src,
        r#"import scala.specialized

class ExternalBox[A](val value: A)

object ConstructorReview {
  def local[@specialized(Int, Long) A](a: A): A = {
    class Captured { def get: A = a }
    class Base(val x: A)
    class Child(y: A) extends Base(y)
    new Child(new Captured().get).x
  }

  def external[@specialized(Int, Long) A](a: A): A = {
    val s = new java.lang.String("x")
    val b = new ExternalBox[A](a)
    if (s.length != 1) throw new RuntimeException("bad")
    b.value
  }
}

object ConstructorReviewMain {
  def main(args: Array[String]): Unit = {
    println(ConstructorReview.local(7))
    println(ConstructorReview.local(8L))
    println(ConstructorReview.local("generic"))
    println(ConstructorReview.external(9))
    println(ConstructorReview.external(10L))
    println(ConstructorReview.external("fallback"))
  }
}
"#,
    )
    .unwrap();
    let expected = "7\n8\ngeneric\n9\n10\nfallback\n";
    let scalac = Path::new("/tmp/scala-2.13.16/bin/scalac");
    let nsc_out = root.join("nsc");
    if scalac.is_file() {
        fs::create_dir_all(&nsc_out).unwrap();
        let status = Command::new(scalac)
            .args(["-d", nsc_out.to_str().unwrap(), src.to_str().unwrap()])
            .status()
            .expect("run nsc constructor specialization provider");
        assert!(
            status.success(),
            "nsc constructor specialization provider failed"
        );
        let run = Command::new("java")
            .args([
                "-Xverify:all",
                "-cp",
                &format!("{}:{}", nsc_out.display(), jar.display()),
                "ConstructorReviewMain",
            ])
            .output()
            .expect("run nsc constructor specialization provider");
        assert!(
            run.status.success(),
            "nsc constructor specialization runtime failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    }

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
        .expect("run scala-rs constructor specialization provider");
    assert!(
        status.success(),
        "constructor specialization provider failed"
    );
    for suffix in ["$1", "$2", "$3"] {
        assert!(
            out.join(format!("ConstructorReview$Base{suffix}.class"))
                .is_file(),
            "missing local Base variant {suffix}"
        );
        assert!(
            out.join(format!("ConstructorReview$Child{suffix}.class"))
                .is_file(),
            "missing local Child variant {suffix}"
        );
    }
    let external_classes: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with("ExternalBox$"))
        .collect();
    assert!(
        external_classes.is_empty(),
        "external constructor was cloned: {external_classes:?}"
    );
    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "ConstructorReviewMain",
        ])
        .output()
        .expect("run scala-rs constructor specialization provider");
    assert!(
        run.status.success(),
        "constructor specialization runtime failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&root);
}

/// Bounds constrain which primitive entries are real ABI members. An upper
/// reference bound keeps the method generic, `Nothing .. AnyVal` admits both
/// primitives, and `Int .. AnyVal` admits Int only. A separately compiled
/// scalac consumer must follow exactly those advertised entries.
#[test]
fn method_specialization_bounds_fixture() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip bounded specialization fixture: scala-library unavailable");
        return;
    };
    let root = tmp_dir("method-specialization-bounds");
    let src = root.join("Bound.scala");
    fs::write(
        &src,
        r#"import scala.specialized

object Bound {
  def upper[@specialized(Int, Long) A <: CharSequence](a: A): Int = a.length
  def lowerNull[@specialized(Int, Long) A >: Null](a: A): String = a.toString
  def mixed[@specialized(Int, Long) A >: Nothing <: AnyVal](a: A): String = a.toString
  def exact[@specialized(Int, Long) A >: Int <: AnyVal](a: A): String = a.toString
  def fallback[@specialized(Int, Long) A](a: A): String = a.toString

  def main(args: Array[String]): Unit = {
    println(upper("hello"))
    println(lowerNull("null-safe"))
    println(mixed(7))
    println(mixed(8L))
    println(exact(9))
    println(fallback("fallback"))
  }
}
"#,
    )
    .unwrap();
    let expected = "5\nnull-safe\n7\n8\n9\nfallback\n";
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
        .expect("run scala-rs bounded specialization provider");
    assert!(status.success(), "bounded specialization provider failed");
    let module = javap_class(&out, "Bound$", &["-p", "-s"]);
    assert!(
        !module.contains("upper$mIc$sp") && !module.contains("upper$mJc$sp"),
        "upper-bound method was specialized: {module}"
    );
    assert!(
        !module.contains("lowerNull$mIc$sp") && !module.contains("lowerNull$mJc$sp"),
        "lower-bound method advertised invalid primitive entries: {module}"
    );
    assert!(
        module.contains("mixed$mIc$sp") && module.contains("mixed$mJc$sp"),
        "valid AnyVal bounds lost primitive entries: {module}"
    );
    assert!(
        module.contains("exact$mIc$sp") && !module.contains("exact$mJc$sp"),
        "mixed bound advertised the wrong primitive entries: {module}"
    );
    assert!(
        module.contains("<A> java.lang.String fallback(A)"),
        "generic fallback disappeared: {module}"
    );
    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Bound",
        ])
        .output()
        .expect("run scala-rs bounded specialization provider");
    assert!(
        run.status.success(),
        "bounded specialization runtime failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);

    let scalac = Path::new("/tmp/scala-2.13.16/bin/scalac");
    if scalac.is_file() {
        let consumer_src = root.join("BoundConsumer.scala");
        fs::write(
            &consumer_src,
            r#"object BoundConsumer {
  def u: Int = Bound.upper("hello")
  def i: String = Bound.mixed(7)
  def l: String = Bound.mixed(8L)
  def e: String = Bound.exact(9)
  def n: String = Bound.lowerNull("null-safe")
  def f: String = Bound.fallback("fallback")

  def main(args: Array[String]): Unit = {
    val result = s"$u:$i:$l:$e:$n:$f"
    if (result != "5:7:8:9:null-safe:fallback") throw new RuntimeException(result)
    println(result)
  }
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
            .expect("run scalac bounded specialization consumer");
        assert!(
            status.success(),
            "scalac bounded specialization consumer failed"
        );
        let consumer = javap_class(&consumer_out, "BoundConsumer$", &["-c", "-p"]);
        assert!(
            consumer.contains("Bound$.upper:(Ljava/lang/CharSequence;)I"),
            "scalac did not retain the generic upper-bound descriptor: {consumer}"
        );
        assert!(
            consumer.contains("Bound$.mixed$mIc$sp:(I)Ljava/lang/String;"),
            "scalac did not select bounded Int entry: {consumer}"
        );
        assert!(
            consumer.contains("Bound$.mixed$mJc$sp:(J)Ljava/lang/String;"),
            "scalac did not select bounded Long entry: {consumer}"
        );
        assert!(
            consumer.contains("Bound$.exact$mIc$sp:(I)Ljava/lang/String;")
                && !consumer.contains("exact$mJc$sp"),
            "scalac selected an unadvertised exact-bound entry: {consumer}"
        );
        assert!(
            !consumer.contains("upper$mIc$sp") && !consumer.contains("upper$mJc$sp"),
            "scalac selected an unadvertised upper-bound entry: {consumer}"
        );
        assert!(
            !consumer.contains("lowerNull$mIc$sp") && !consumer.contains("lowerNull$mJc$sp"),
            "scalac selected an unadvertised lower-bound entry: {consumer}"
        );
        let consumer_run = Command::new("java")
            .args([
                "-Xverify:all",
                "-cp",
                &format!(
                    "{}:{}:{}",
                    consumer_out.display(),
                    out.display(),
                    jar.display()
                ),
                "BoundConsumer",
            ])
            .output()
            .expect("run scalac bounded specialization consumer");
        assert!(
            consumer_run.status.success(),
            "scalac bounded specialization consumer runtime failed: {}",
            String::from_utf8_lossy(&consumer_run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&consumer_run.stdout),
            "5:7:8:9:null-safe:fallback\n"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
