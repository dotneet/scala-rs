//! A pickle too big for one `CONSTANT_Utf8` (SID-10 `ScalaLongSignature`).
//!
//! JVMS §4.4.7 gives a `CONSTANT_Utf8_info` a `u2` byte count, so the writer's
//! `encoded.len() as u16` silently wrapped once a signature passed 64K and
//! left a constant pool no reader could walk: `slick/util/TupleMethods`
//! became "unexpected tag at #104" the moment its nested classes went into
//! its signature. nsc's answer -- and now ours -- is to split the encoded
//! string across an array of constants under `ScalaLongSignature`, which the
//! reader concatenates before decoding.

use scala_rs_backend::classfile::{ClassEmit, Pool, ACC_PUBLIC, ACC_SUPER};
use scala_rs_backend::pickle::encode_to_annotation_string;

fn emit_with_signature(raw: &[u8]) -> Vec<u8> {
    let c = ClassEmit {
        access: ACC_PUBLIC | ACC_SUPER,
        this_name: "Big".into(),
        super_name: "java/lang/Object".into(),
        interfaces: Vec::new(),
        fields: Vec::new(),
        methods: Vec::new(),
        source: "Big.scala".into(),
        scala_signature: Some(encode_to_annotation_string(raw)),
        scala_raw: false,
        inner_classes: Vec::new(),
        enclosing_method: None,
        signature: None,
        field_signatures: Default::default(),
        field_constants: Default::default(),
    };
    c.write_with_pool(Pool::new()).expect("write class file")
}

/// Deterministic bytes that exercise every value, including the `0x7f` the
/// encoder turns into a `\0` char (two bytes in modified UTF-8, which is why
/// the split counts bytes and not chars).
fn pickle_bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 256) as u8).collect()
}

#[test]
fn a_signature_over_64k_survives_the_round_trip() {
    let raw = pickle_bytes(200_000);
    assert!(
        encode_to_annotation_string(&raw).chars().count() > 65_535,
        "test needs a signature past the one-constant limit"
    );
    let bytes = emit_with_signature(&raw);
    let back =
        scala_rs_pickle::scala_signature_bytes(&bytes).expect("ScalaLongSignature bytes read back");
    assert_eq!(back, raw, "pickle bytes changed across the split");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("Lscala/reflect/ScalaLongSignature;"),
        "expected the long form for an oversized signature"
    );
}

#[test]
fn a_small_signature_still_uses_the_single_constant_form() {
    let raw = pickle_bytes(64);
    let bytes = emit_with_signature(&raw);
    let back =
        scala_rs_pickle::scala_signature_bytes(&bytes).expect("ScalaSignature bytes read back");
    assert_eq!(back, raw);
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("Lscala/reflect/ScalaSignature;"), "short form");
    assert!(
        !s.contains("Lscala/reflect/ScalaLongSignature;"),
        "short form"
    );
}

/// The boundary itself: a signature that fits exactly, and one byte past it.
#[test]
fn the_split_only_kicks_in_past_the_limit() {
    for n in [40_000usize, 57_000, 57_500, 58_000] {
        let raw = pickle_bytes(n);
        let enc = encode_to_annotation_string(&raw);
        let utf8_len: usize = enc.chars().map(|c| if c as u32 == 0 { 2 } else { 1 }).sum();
        let bytes = emit_with_signature(&raw);
        let back = scala_rs_pickle::scala_signature_bytes(&bytes)
            .unwrap_or_else(|| panic!("no signature read back for n={n}"));
        assert_eq!(back, raw, "pickle bytes changed for n={n}");
        let s = String::from_utf8_lossy(&bytes);
        let long = s.contains("Lscala/reflect/ScalaLongSignature;");
        assert_eq!(
            long,
            utf8_len > 65_535,
            "wrong form for n={n} ({utf8_len} bytes)"
        );
    }
}
