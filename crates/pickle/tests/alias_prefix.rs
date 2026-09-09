//! Constructor-relevant prefixes survive the alias type view.
use scala_rs_pickle::sym::{class_sigs, SigType};
use scala_rs_pickle::{read::read_pickle, scala_signature_bytes};
#[test]
fn alias_retains_enclosing_this_from_real_scalac() {
    let dir = std::env::temp_dir().join(format!(
        "pickle-alias-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("Lib.scala");
    std::fs::write(&source, "package prefixprobe\ntrait Owner { self => class Base[A]; trait API { type Alias[A] = self.Base[A] }; object api extends API }; object P extends Owner; object Holder { val other = P.api }\n").unwrap();
    let output = std::process::Command::new("/tmp/scala-2.13.16/bin/scalac")
        .arg("-d")
        .arg(&dir)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let holder_bytes = std::fs::read(dir.join("prefixprobe/Holder.class")).unwrap();
    let holder_raw = scala_signature_bytes(&holder_bytes).unwrap();
    let holder_pickle = read_pickle(&holder_raw).unwrap();
    let holder_sigs = class_sigs(&holder_pickle);
    let holder = holder_sigs
        .iter()
        .find(|s| s.full_name == "prefixprobe.Holder")
        .unwrap();
    let other = holder.members.iter().find(|m| m.name == "other").unwrap();
    let result = match &other.ty {
        SigType::Poly { tparams, result } if tparams.is_empty() => &**result,
        ty => ty,
    };
    assert_eq!(
        result,
        &SigType::Single {
            prefix: Box::new(SigType::Single {
                prefix: Box::new(SigType::This("prefixprobe".into())),
                sym: "prefixprobe.P".into(),
            }),
            sym: "prefixprobe.Owner.api".into(),
        }
    );
    let bytes = std::fs::read(dir.join("prefixprobe/Owner.class")).unwrap();
    let raw = scala_signature_bytes(&bytes).unwrap();
    let pickle = read_pickle(&raw).unwrap();
    let sigs = class_sigs(&pickle);
    let api = sigs
        .iter()
        .find(|s| s.full_name == "prefixprobe.Owner.API")
        .unwrap();
    let alias = api.members.iter().find(|m| m.name == "Alias").unwrap();
    assert_eq!(
        alias.alias_prefix,
        Some(SigType::This("prefixprobe.Owner".into()))
    );
}
