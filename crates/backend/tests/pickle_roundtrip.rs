//! The backend's pickle *writer* against the shared pickle *reader*.
//!
//! These live here rather than in `scala-rs-pickle` because they drive the
//! writer (`scala_rs_backend::pickle`) and the typer, neither of which the
//! pickle crate depends on.

use scala_rs_backend::pickle;
use scala_rs_pickle::read::{read_pickle, tags, Entry};
use scala_rs_pickle::sym::{class_sigs, render, MemberKind};

#[test]
fn tag_table_matches_the_writer() {
    assert_eq!(tags::TERMname, pickle::TERMNAME);
    assert_eq!(tags::TYPEname, pickle::TYPENAME);
    assert_eq!(tags::NONEsym, pickle::NONESYM);
    assert_eq!(tags::TYPEsym, pickle::TYPESYM);
    assert_eq!(tags::ALIASsym, pickle::ALIASSYM);
    assert_eq!(tags::CLASSsym, pickle::CLASSSYM);
    assert_eq!(tags::MODULEsym, pickle::MODULESYM);
    assert_eq!(tags::VALsym, pickle::VALSYM);
    assert_eq!(tags::EXTref, pickle::EXTREF);
    assert_eq!(tags::EXTMODCLASSref, pickle::EXTMODCLASSREF);
    assert_eq!(tags::NOtpe, pickle::NOTPE);
    assert_eq!(tags::NOPREFIXtpe, pickle::NOPREFIXTPE);
    assert_eq!(tags::THIStpe, pickle::THISTPE);
    assert_eq!(tags::SINGLEtpe, pickle::SINGLETPE);
    assert_eq!(tags::CONSTANTtpe, pickle::CONSTANTtpe);
    assert_eq!(tags::TYPEREFtpe, pickle::TYPEREFTPE);
    assert_eq!(tags::TYPEBOUNDStpe, pickle::TYPEBOUNDSTPE);
    assert_eq!(tags::REFINEDtpe, pickle::REFINEDTPE);
    assert_eq!(tags::CLASSINFOtpe, pickle::CLASSINFOTPE);
    assert_eq!(tags::METHODtpe, pickle::METHODTPE);
    assert_eq!(tags::POLYtpe, pickle::POLYTPE);
    assert_eq!(tags::LITERALunit, pickle::LITERALunit);
    assert_eq!(tags::LITERALstring, pickle::LITERALstring);
    assert_eq!(tags::LITERALclass, pickle::LITERALclass);
    assert_eq!(tags::SYMANNOT, pickle::SYMANNOT);
    assert_eq!(tags::ANNOTATEDtpe, pickle::ANNOTATEDTPE);
    assert_eq!(tags::ANNOTINFO, pickle::ANNOTINFO);
    assert_eq!(tags::EXISTENTIALtpe, pickle::EXISTENTIALTPE);
    assert_eq!(tags::TREE, pickle::TREE);
    assert_eq!(tags::APPLYtree, pickle::APPLYtree);
    assert_eq!(tags::SELECTtree, pickle::SELECTtree);
    assert_eq!(tags::IDENTtree, pickle::IDENTtree);
    assert_eq!(tags::LITERALtree, pickle::LITERALtree);
    assert_eq!(tags::THIStree, pickle::THIStree);
    assert_eq!(tags::SUPERtree, pickle::SUPERtree);
    assert_eq!(tags::TYPEAPPLYtree, pickle::TYPEAPPLYtree);
}

#[test]
fn reads_pickles_written_by_our_own_writer() {
    // Covers CLASSsym / MODULEsym / VALsym / TYPEsym / ALIASsym, POLYtpe,
    // METHODtpe, TYPEREFtpe, EXISTENTIALtpe, REFINEDtpe, CONSTANTtpe,
    // ANNOTATEDtpe and SYMANNOT through the writer we already have.
    let src = r#"
trait Show { def show: String }
trait Named { def name: String }
class Box[A](val get: A) {
  type Elem = A
  def map[B](f: A => B): Box[B] = new Box(f(get))
  def raw(xs: List[_]): Int = 0
  def both: Show with Named = null
  def self0: this.type = this
}
case class Point(x: Int, y: Int)
object Lib {
  type Alias = String
  val n: Int = 1
  @deprecated("gone", "2.13.0") def old(s: String): String = s
  def lit(x: 1): Int = 0
}
"#;
    let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
    assert!(
        !scala_rs_typer::has_errors(&diags),
        "type errors: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let pickles = pickle::pickle_all(&st);
    assert!(!pickles.is_empty(), "expected pickles");
    let mut seen_class = false;
    let mut seen_alias = false;
    for raw in pickles.values() {
        // Go through the annotation-string encoding, like a real classfile.
        let enc = pickle::encode_to_annotation_string(raw);
        let dec = pickle::decode_annotation_string(&enc);
        let p = read_pickle(&dec).unwrap_or_else(|e| panic!("our own pickle: {e}"));
        assert_eq!(p.major, pickle::MAJOR);
        for e in &p.entries {
            match e {
                Entry::ClassSym { info, .. } if p.name(info.name) == Some("Box") => {
                    seen_class = true;
                }
                Entry::AliasSym(info) if p.name(info.name) == Some("Alias") => {
                    seen_alias = true;
                }
                _ => {}
            }
        }
    }
    assert!(seen_class, "Box CLASSsym not read back");
    assert!(seen_alias, "Lib.Alias ALIASsym not read back");
}

fn sigs_of(src: &str) -> Vec<scala_rs_pickle::ClassSig> {
    let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
    assert!(
        !scala_rs_typer::has_errors(&diags),
        "type errors: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let mut out = Vec::new();
    for raw in pickle::pickle_all(&st).values() {
        let p = read_pickle(raw).expect("read our own pickle");
        out.extend(class_sigs(&p));
    }
    out
}

#[test]
fn recovers_a_polymorphic_method_signature() {
    let sigs = sigs_of(
        r#"
class Box[A](val get: A) {
  def map[B](f: A => B): Box[B] = new Box(f(get))
  def size: Int = 1
}
"#,
    );
    let boxc = sigs
        .iter()
        .find(|c| c.full_name == "Box" && !c.is_module)
        .expect("Box");
    assert_eq!(boxc.tparams.len(), 1);
    assert_eq!(boxc.tparams[0].name, "A");
    let map = boxc.member("map").expect("Box#map");
    assert_eq!(map.kind, MemberKind::Def);
    let rendered = render(&map.ty);
    assert!(rendered.starts_with("[B](f: "), "{rendered}");
    assert!(rendered.ends_with("Box[B]"), "{rendered}");
    // A parameterless `def` stays a (nullary) method, not a val.
    let size = boxc.member("size").expect("Box#size");
    assert_eq!(render(&size.ty), "=> scala.Int");
    assert_eq!(size.kind, MemberKind::Def);
}

#[test]
fn parents_and_module_classes_are_recovered() {
    let sigs = sigs_of(
        r#"
trait Show { def show: String }
class Impl extends Show { def show: String = "" }
object Impl { val tag: String = "i" }
"#,
    );
    let imp = sigs
        .iter()
        .find(|c| c.full_name == "Impl" && !c.is_module)
        .expect("Impl class");
    assert!(
        !imp.parents.is_empty(),
        "expected at least one parent for Impl"
    );
    assert!(imp.member("show").is_some(), "Impl#show");
    let obj = sigs
        .iter()
        .find(|c| c.full_name == "Impl" && c.is_module)
        .expect("Impl module class");
    assert!(obj.member("tag").is_some(), "Impl.tag");
}
