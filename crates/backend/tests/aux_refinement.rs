//! Pickle compatibility for the higher-kinded `Aux` refinement idiom.

use scala_rs_backend::pickle;
use scala_rs_pickle::read::{pflags, read_pickle, Entry};
use scala_rs_pickle::sym::{class_sigs, render, SigType};

const AUX_SOURCE: &str = r#"
trait P[M[_]] { type F[_] }
object P { type Aux[M[_], F0[_]] = P[M] { type F[x] = F0[x] } }
"#;

#[test]
fn refined_aux_alias_is_direct_and_overridden() {
    let (_tree, st, diags) = scala_rs_typer::typecheck_str(AUX_SOURCE);
    assert!(
        !scala_rs_typer::has_errors(&diags),
        "type errors: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let mut aux_renderings = Vec::new();
    let mut refined_tparams = 0;
    for raw in pickle::pickle_all(&st).values() {
        let p = read_pickle(raw).expect("our Aux pickle should be readable");
        for entry in &p.entries {
            let Entry::TypeSym(info) = entry else {
                continue;
            };
            if p.name(info.name) != Some("x$0") {
                continue;
            }
            refined_tparams += 1;
            assert!(info.has(pflags::PARAM), "refined F's x must be a PARAM");
            assert!(
                info.has(pflags::DEFERRED),
                "refined F's x must retain its type-parameter bounds"
            );
            assert!(
                !info.has(pflags::FINAL),
                "refined F's x must not be pickled as FINAL"
            );
        }
        for member in class_sigs(&p)
            .into_iter()
            .flat_map(|sig| sig.members.into_iter())
            .filter(|member| member.name == "Aux")
        {
            if let Some(flags) = refined_member_flags(&member.ty, "F") {
                assert!(
                    flags & pflags::OVERRIDE != 0,
                    "Aux's refined F must carry nsc OVERRIDE"
                );
            }
            aux_renderings.push(render(&member.ty));
        }
    }
    assert!(
        refined_tparams > 0,
        "expected F's fresh parameter in at least one published pickle"
    );
    assert!(!aux_renderings.is_empty(), "missing P.Aux alias");
    aux_renderings.sort();
    aux_renderings.dedup();
    assert_eq!(
        aux_renderings,
        vec!["[M, F0]P[M] { F: [x$0]F0[x$0] }"],
        "Aux's RHS should be the direct F0[x] lambda"
    );
}

fn refined_member_flags(ty: &SigType, name: &str) -> Option<u64> {
    match ty {
        SigType::Poly { result, .. } => refined_member_flags(result, name),
        SigType::Refined { parents, decls } => decls
            .iter()
            .find_map(|member| (member.name == name).then_some(member.flags))
            .or_else(|| {
                parents
                    .iter()
                    .find_map(|parent| refined_member_flags(parent, name))
            }),
        _ => None,
    }
}
