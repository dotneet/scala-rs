//! Two independent codegen bugs, both found by running real-world code
//! through the compiler and checking the *shape* of what came out, not just
//! whether it type-checked.
//!
//! 1. A `private` trait method went out on the interface as
//!    `ACC_PRIVATE | ACC_ABSTRACT` -- illegal under JVMS 4.6 (no method may
//!    combine `private` and `abstract`, interface ones included).
//!    `tests/slick_subset.sh` (which loads every classfile a real slick
//!    build produces with `-Xverify:all`) caught it on
//!    `slick.util.ReadAheadIterator`, whose `private[this] def update()` is
//!    called only from other members of the same trait.
//!
//!    Real nsc keeps a `private` trait method's whole body directly inside
//!    the interface, as a genuine `private` *instance* method. Since
//!    `agent/traitclass` this backend also puts trait bodies on the
//!    interface, but a `private` one cannot be a `default` method (nothing
//!    outside may reach it, and a mixin forwarder must not exist for it), so
//!    it keeps the `$this`-taking shape the other bodies had: a `private
//!    static <name>$` on the interface, reached from the trait's other
//!    members with a same-class `invokestatic` instead of
//!    `invokeinterface`. The invariant is nsc's -- no interface declaration
//!    and no mixin forwarder -- and only the calling convention differs.
//!
//!    See `is_trait_private_def` in `crates/backend/src/gen.rs` and its
//!    four call sites (the interface's abstract-method loop,
//!    `emit_trait_impl_method`'s access flags and shape, and the two places
//!    that resolve a "next implementation in the linearization" --
//!    `next_lin_impl` and `emit_mixin_forwarders`).
//!
//! 2. `extends` a generic superclass with a primitive constructor argument
//!    (`class A1 extends java.util.concurrent.atomic.AtomicReference[Int](1)`,
//!    found live by `agent/javanest`) left the primitive unboxed on the
//!    stack where the erased `<init>` wanted an `Object` --
//!    `VerifyError: Type integer ... is not assignable to 'java/lang/Object'`.
//!    `gen_new` already made this same box check for an ordinary `new`; the
//!    `super_args` loops that build a class's or a module's own `<init>`
//!    (`emit_class` / the module `<init>` builder in `crates/backend/src/gen.rs`)
//!    did not. `parent_super_ctor` now also returns the constructor's
//!    *declared* parameter types (`ctor_param_tys`, shared with `gen_new`),
//!    and both loops box a primitive argument exactly when `gen_new` would.
//!
//! Fixtures are dual-run: against the real `scala-library` jar and, where
//! the private runtime can back them, on it -- under `-Xverify:all`, with
//! the stdout nsc 2.13.16 prints for the same source.

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
        "scala-rs-traitpriv-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile(out: &Path, jar: Option<&Path>, srcs: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    match jar {
        Some(j) => cmd.args(["--scala-library", j.to_str().unwrap()]),
        None => cmd.arg("--no-scala-library"),
    };
    let output = cmd.output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Compile and run a fixture in both modes and compare with nsc's stdout.
fn dual_run(name: &str) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let exp = expected(name);

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, std::slice::from_ref(&src));
    assert!(ok, "compile {name} (private runtime) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&priv_out, None),
            exp,
            "stdout mismatch for {name} on the private runtime"
        );
    }

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} (jar): scala-library jar not present");
        return priv_out;
    };
    let jar_out = tmp_dir("jar");
    let (ok, msgs) = compile(&jar_out, Some(&jar), &[src]);
    assert!(ok, "compile {name} (jar) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&jar_out, Some(&jar)),
            exp,
            "stdout mismatch for {name} against the jar"
        );
    }
    let _ = fs::remove_dir_all(&jar_out);
    priv_out
}

// ----------------------------------------------------------- classfile shape
//
// A minimal JVMS 4 reader: just enough to answer "does method `name` exist,
// and with which access flags" for one classfile. `javap` disassembles the
// same information but folds `private abstract` into a shape that still
// *reads* like a normal declaration, so the shape assertions below parse the
// bytes directly instead.

struct ClassFile {
    methods: Vec<(String, u16)>, // (name, access_flags)
}

fn read_class(path: &Path) -> ClassFile {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let u16at = |i: usize| u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32at = |i: usize| {
        u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize
    };
    let count = u16at(8);
    let mut at: Vec<(u8, usize)> = vec![(0, 0); count];
    let mut p = 10;
    let mut i = 1;
    while i < count {
        let tag = bytes[p];
        at[i] = (tag, p + 1);
        p += 1 + match tag {
            1 => 2 + u16at(p + 1),
            7 | 8 | 16 | 19 | 20 => 2,
            15 => 3,
            5 | 6 => {
                i += 1; // longs and doubles take two constant-pool slots
                8
            }
            _ => 4,
        };
        i += 1;
    }
    let utf8 = |idx: usize| {
        let (tag, off) = at[idx];
        assert_eq!(tag, 1, "constant #{idx} is not Utf8");
        let len = u16::from_be_bytes([bytes[off], bytes[off + 1]]) as usize;
        String::from_utf8_lossy(&bytes[off + 2..off + 2 + len]).into_owned()
    };
    // access_flags(2) this_class(2) super_class(2) interfaces_count(2)
    let ifcount = u16at(p + 6);
    p += 8 + ifcount * 2;
    let fcount = u16at(p);
    p += 2;
    for _ in 0..fcount {
        let attr_count = u16at(p + 6);
        p += 8;
        for _ in 0..attr_count {
            let alen = u32at(p + 2);
            p += 6 + alen;
        }
    }
    let mcount = u16at(p);
    p += 2;
    let mut methods = Vec::with_capacity(mcount);
    for _ in 0..mcount {
        let macc = u16at(p) as u16;
        let mname = utf8(u16at(p + 2));
        let attr_count = u16at(p + 6);
        p += 8;
        for _ in 0..attr_count {
            let alen = u32at(p + 2);
            p += 6 + alen;
        }
        methods.push((mname, macc));
    }
    ClassFile { methods }
}

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PRIVATE: u16 = 0x0002;
const ACC_STATIC: u16 = 0x0008;
const ACC_ABSTRACT: u16 = 0x0400;

fn method_flags(cf: &ClassFile, name: &str) -> Option<u16> {
    cf.methods.iter().find(|(n, _)| n == name).map(|(_, f)| *f)
}

// --------------------------------------------------------------- (1) tp1/tp2/tp3

#[test]
fn tp1_trait_private_state_matches_scalac() {
    dual_run("tp1");
}

/// The reported shape bug itself: a `private[this]` trait method must not
/// carry `ACC_ABSTRACT` on the interface -- in fact it must not be *declared*
/// there at all -- and its real body is a `private static <name>$` on the
/// interface, not a `public` one and not a `default` method.
#[test]
fn tp1_private_method_is_not_abstract_on_the_interface() {
    let out = dual_run("tp1");
    let iface = read_class(&out.join("ReadAheadIterator.class"));
    assert!(
        method_flags(&iface, "update").is_none(),
        "`update` must not be declared on the interface at all, got: {:?}",
        iface.methods
    );
    // The public members are still there: `hasNext` is concrete in the
    // trait, so it is a public `default` method with its body, plus the
    // `hasNext$` static every mixin forwarder calls.
    let hn = method_flags(&iface, "hasNext").expect("hasNext missing from interface");
    assert_eq!(hn & (ACC_PUBLIC | ACC_ABSTRACT | ACC_STATIC), ACC_PUBLIC);
    let hns = method_flags(&iface, "hasNext$").expect("hasNext$ missing from interface");
    assert_eq!(hns & (ACC_PUBLIC | ACC_STATIC), ACC_PUBLIC | ACC_STATIC);

    let upd = method_flags(&iface, "update$").expect("update$ missing from the interface");
    assert_eq!(
        upd & (ACC_PRIVATE | ACC_STATIC | ACC_ABSTRACT),
        ACC_PRIVATE | ACC_STATIC,
        "update$ must be `private static`, not abstract or public, got flags {upd:#x}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn tp2_two_traits_private_same_name_matches_scalac() {
    dual_run("tp2");
}

/// Neither trait's private `helper` may get a mixin forwarder on `Both`: a
/// forwarder there would have to pick one trait's body arbitrarily
/// (silently shadowing the other), or collide.
#[test]
fn tp2_private_method_gets_no_mixin_forwarder() {
    let out = dual_run("tp2");
    let both = read_class(&out.join("Both.class"));
    assert!(
        method_flags(&both, "helper").is_none(),
        "Both must not carry a forwarder for either trait's private helper, got: {:?}",
        both.methods
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn tp3_widened_private_still_public_abstract() {
    dual_run("tp3");
}

/// The regression this guards: `is_trait_private_def` must say "no" once the
/// typer has widened a `private` member for companion access, so `secret`
/// keeps its ordinary interface signature instead of silently vanishing like
/// `tp1`'s truly-private `update`.
///
/// The name it keeps is the *expanded* one. Publishing a `private` member
/// under its source name is what let a subclass override it by accident, so
/// `expand_private_names` renames it the way scalac does -- `javap -p` on
/// scalac 2.13.16's own `Widened.class` for this very fixture reports
/// `public default int Widened$$secret()`, not `secret()`.
#[test]
fn tp3_widened_private_keeps_interface_signature() {
    let out = dual_run("tp3");
    let iface = read_class(&out.join("Widened.class"));
    assert!(
        method_flags(&iface, "secret").is_none(),
        "the source name must not be published: {:?}",
        iface.methods
    );
    let flags = method_flags(&iface, "Widened$$secret").unwrap_or_else(|| {
        panic!(
            "widened `secret` must still be on the interface, got: {:?}",
            iface.methods
        )
    });
    assert_eq!(
        flags & (ACC_PUBLIC | ACC_ABSTRACT | ACC_PRIVATE | ACC_STATIC),
        ACC_PUBLIC,
        "widened secret must be a public default method, not private and not \
         abstract, got flags {flags:#x}"
    );
    let fwd = method_flags(&iface, "Widened$$secret$")
        .expect("a widened private still gets its `m$` static");
    assert_eq!(fwd & (ACC_PUBLIC | ACC_STATIC), ACC_PUBLIC | ACC_STATIC);
    let _ = fs::remove_dir_all(&out);
}

// --------------------------------------------------------------------- (2)

/// `class ... extends <Java generic>(<primitive literal>)`.
#[test]
fn tp4_java_generic_super_ctor_arg_is_boxed() {
    dual_run("tp4");
}

/// A self-authored Scala generic superclass, `object ... extends`, and every
/// JVM primitive kind through both.
#[test]
fn tp5_scala_generic_super_ctor_arg_boxed_all_primitives() {
    dual_run("tp5");
}
