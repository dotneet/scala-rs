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

/// A nested class is written into its *owner's* pickle, not only into its own.
///
/// scala-rs emits one `ScalaSignature` per class file, so
/// `Support$Row.class` always carried a perfectly good pickle of `Row`. nsc
/// never opens it: it resolves `Support.Row` as a member of `Support`'s
/// signature, and `Support`'s signature declared nothing. Real scalac
/// reading our slick output stopped at "Symbol 'type
/// slick.jdbc.JdbcActionComponent.MultipleRowsPerStatementSupport' is
/// missing from the classpath" the first time a parent list mentioned one.
///
/// The per-class-file pickles stay exactly as they were -- this compiler's
/// own reader still finds each nested class in its own class file.
#[test]
fn a_nested_class_is_declared_in_its_owners_pickle() {
    let src = r#"
object Support {
  trait Rows { def rows: Int }
  class Row(val n: Int)
}
class Outer {
  class Inner { def v: Int = 1 }
  object Cfg { val k: Int = 1 }
}
"#;
    let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
    assert!(
        !scala_rs_typer::has_errors(&diags),
        "type errors: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    for raw in pickle::pickle_all(&st).values() {
        let p = read_pickle(raw).expect("read our own pickle");
        for (i, e) in p.entries.iter().enumerate() {
            let Entry::ClassSym { info, .. } = e else {
                continue;
            };
            let Some(outer) = p.name(info.name) else {
                continue;
            };
            if outer != "Support" && outer != "Outer" {
                continue;
            }
            let nested: Vec<String> = p
                .entries
                .iter()
                .filter_map(|m| match m {
                    Entry::ClassSym { info: mi, .. } if mi.owner == i as u32 => {
                        p.name(mi.name).map(str::to_string)
                    }
                    _ => None,
                })
                .collect();
            found.push((outer.to_string(), nested));
        }
    }
    let support = found
        .iter()
        .find(|(n, ms)| n == "Support" && !ms.is_empty())
        .unwrap_or_else(|| panic!("no pickle declares Support's members: {found:?}"));
    assert!(support.1.iter().any(|n| n == "Rows"), "{found:?}");
    assert!(support.1.iter().any(|n| n == "Row"), "{found:?}");
    let outer = found
        .iter()
        .find(|(n, ms)| n == "Outer" && !ms.is_empty())
        .unwrap_or_else(|| panic!("no pickle declares Outer's members: {found:?}"));
    assert!(outer.1.iter().any(|n| n == "Inner"), "{found:?}");
    // A nested `object` goes in as its module class, so the term half nsc
    // binds `Outer.Cfg` from is there too.
    assert!(outer.1.iter().any(|n| n == "Cfg"), "{found:?}");
}

/// `def f(): T` and `def f: T` are two different types, and the pickle has to
/// keep them apart.
///
/// nsc writes the first as a `METHODtpe` over an empty parameter list and the
/// second as a `POLYtpe` with no type parameters (`NullaryMethodType`), and
/// holds the call site to the difference: `f()` against the second is
/// `Int does not take parameters`. We used to write both as the `POLYtpe`, so
/// every zero-argument method we emitted -- a zero-field case class's
/// `apply()` included -- was uncallable with the parentheses its own source
/// wrote.
#[test]
fn an_empty_parameter_list_is_not_a_nullary_method() {
    let sigs = sigs_of(
        r#"
trait Ord[A]
case class Empty()
class Zero {
  def run(): Int = 1
  def value: Int = 2
  def withImplicit()(implicit ord: Ord[Int]): Int = 0
  def curried(a: Int)(b: Int): Int = a + b
  def generic[A](a: A)(): Int = 0
}
"#,
    );
    let zero = sigs
        .iter()
        .find(|c| c.full_name == "Zero" && !c.is_module)
        .expect("Zero");
    assert_eq!(render(&zero.member("run").expect("run").ty), "()scala.Int");
    assert_eq!(
        render(&zero.member("value").expect("value").ty),
        "=> scala.Int"
    );
    // The clauses `uncurry` joins off the symbol are pickled the way nsc
    // pickles them, which is before uncurry runs: nested `METHODtpe`s.
    assert_eq!(
        render(&zero.member("withImplicit").expect("withImplicit").ty),
        "()(implicit ord: Ord[scala.Int])scala.Int"
    );
    assert_eq!(
        render(&zero.member("curried").expect("curried").ty),
        "(a: scala.Int)(b: scala.Int)scala.Int"
    );
    assert_eq!(
        render(&zero.member("generic").expect("generic").ty),
        "[A](a: A)()scala.Int"
    );
    // The zero-field case class, whose `apply()` is what real scalac rejected.
    let empty_obj = sigs
        .iter()
        .find(|c| c.full_name == "Empty" && c.is_module)
        .expect("Empty module class");
    assert_eq!(
        render(&empty_obj.member("apply").expect("apply").ty),
        "()Empty"
    );
    let empty = sigs
        .iter()
        .find(|c| c.full_name == "Empty" && !c.is_module)
        .expect("Empty class");
    assert_eq!(render(&empty.member("copy").expect("copy").ty), "()Empty");
}

/// A default getter with no preceding clause is nullary, like nsc's.
///
/// Pickled as `copy$default$1()` instead, real scalac accepts the class file
/// but warns "Auto-application to `()` is deprecated" at every call that
/// omits a `copy` argument -- and Scala 3 would eta-expand it instead.
#[test]
fn a_default_getter_with_no_preceding_clause_is_nullary() {
    let sigs = sigs_of(
        r#"
case class Pair(a: Int, b: Int)
class Curried { def f(a: Int)(b: Int = a): Int = a + b }
"#,
    );
    let pair = sigs
        .iter()
        .find(|c| c.full_name == "Pair" && !c.is_module)
        .expect("Pair");
    for n in ["copy$default$1", "copy$default$2"] {
        assert_eq!(
            render(&pair.member(n).unwrap_or_else(|| panic!("{n}")).ty),
            "=> scala.Int",
            "{n}"
        );
    }
    // A later clause's getter does take the clauses before it.
    let curried = sigs
        .iter()
        .find(|c| c.full_name == "Curried" && !c.is_module)
        .expect("Curried");
    assert_eq!(
        render(&curried.member("f$default$2").expect("f$default$2").ty),
        "(a: scala.Int)scala.Int"
    );
}
