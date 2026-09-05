//! Reading `@specialized` / `@unspecialized`.
//!
//! This is the first half of specialization ("accept and record"): it turns an
//! annotation tree into the set of types the annotation selects, so the symbol
//! table can carry it. It does **not** specialize anything — no `Foo$mcI$sp`
//! class is emitted, and `tests/spec_classfiles.sh` is the ledger that says so.
//! See `docs/specialization.md`.
//!
//! What the argument list means is nsc's `SpecializeTypes.specializedOn`:
//!
//! * no arguments at all — `@specialized T` — selects every primitive value
//!   class, nsc's `specializableTypes`;
//! * a `Specializable` group — `@specialized(Specializable.Primitives)`, or
//!   plain `@specialized(Primitives)` under `import Specializable._` — selects
//!   the group's members, as spelled out in `scala/Specializable.scala`;
//! * anything else is read as a type name, so `@specialized(Int, AnyRef)`
//!   selects those two.
//!
//! A name that is neither a value class nor a group selects nothing, which is
//! also what nsc ends up doing: `specializedOn` collects the argument's type
//! symbol and the specializer then only looks for it among the value classes.

use crate::ast::{Tree, TreeKind};

/// The simple name of `scala.specialized`, after import renames are undone.
pub const SPECIALIZED: &str = "specialized";
/// The simple name of `scala.annotation.unspecialized`.
pub const UNSPECIALIZED: &str = "unspecialized";

/// A type `@specialized` can select: the nine primitive value classes, plus
/// `AnyRef`, which the `Everything` and `BestOfBreed` groups include.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpecializedType {
    Byte,
    Short,
    Char,
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Unit,
    AnyRef,
}

impl SpecializedType {
    /// Declaration order, which is also the order [`SpecializedTypes::iter`]
    /// yields and therefore the order a diagnostic prints.
    pub const ALL: [SpecializedType; 10] = [
        SpecializedType::Byte,
        SpecializedType::Short,
        SpecializedType::Char,
        SpecializedType::Int,
        SpecializedType::Long,
        SpecializedType::Float,
        SpecializedType::Double,
        SpecializedType::Boolean,
        SpecializedType::Unit,
        SpecializedType::AnyRef,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SpecializedType::Byte => "Byte",
            SpecializedType::Short => "Short",
            SpecializedType::Char => "Char",
            SpecializedType::Int => "Int",
            SpecializedType::Long => "Long",
            SpecializedType::Float => "Float",
            SpecializedType::Double => "Double",
            SpecializedType::Boolean => "Boolean",
            SpecializedType::Unit => "Unit",
            SpecializedType::AnyRef => "AnyRef",
        }
    }

    /// nsc's `$mc<letter>$sp` tag for this type. Stage 2 builds specialized
    /// names out of these; stage 1 keeps them here so the ledger script and
    /// this table cannot drift apart.
    pub fn tag(self) -> char {
        match self {
            SpecializedType::Byte => 'B',
            SpecializedType::Short => 'S',
            SpecializedType::Char => 'C',
            SpecializedType::Int => 'I',
            SpecializedType::Long => 'J',
            SpecializedType::Float => 'F',
            SpecializedType::Double => 'D',
            SpecializedType::Boolean => 'Z',
            SpecializedType::Unit => 'V',
            SpecializedType::AnyRef => 'L',
        }
    }

    fn from_name(name: &str) -> Option<SpecializedType> {
        SpecializedType::ALL.into_iter().find(|t| t.name() == name)
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// The set one `@specialized` selects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpecializedTypes(u16);

impl SpecializedTypes {
    pub const EMPTY: SpecializedTypes = SpecializedTypes(0);

    /// nsc's `specializableTypes`: the nine primitive value classes, which is
    /// what a bare `@specialized` selects. `AnyRef` is deliberately not in it.
    pub fn primitives() -> SpecializedTypes {
        SpecializedTypes::of(&[
            SpecializedType::Byte,
            SpecializedType::Short,
            SpecializedType::Char,
            SpecializedType::Int,
            SpecializedType::Long,
            SpecializedType::Float,
            SpecializedType::Double,
            SpecializedType::Boolean,
            SpecializedType::Unit,
        ])
    }

    pub fn of(types: &[SpecializedType]) -> SpecializedTypes {
        let mut s = SpecializedTypes::EMPTY;
        for t in types {
            s.insert(*t);
        }
        s
    }

    pub fn insert(&mut self, t: SpecializedType) {
        self.0 |= t.bit();
    }

    pub fn contains(self, t: SpecializedType) -> bool {
        self.0 & t.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn len(self) -> u32 {
        self.0.count_ones()
    }

    pub fn iter(self) -> impl Iterator<Item = SpecializedType> {
        SpecializedType::ALL
            .into_iter()
            .filter(move |t| self.contains(*t))
    }

    /// The selected names in declaration order — what a test or a diagnostic
    /// prints.
    pub fn names(self) -> Vec<&'static str> {
        self.iter().map(|t| t.name()).collect()
    }
}

/// The `Specializable` groups, verbatim from `scala/Specializable.scala`.
/// `Unit` is both a group and a value class, and both readings select `Unit`,
/// so the ambiguity has no consequence.
fn group(name: &str) -> Option<&'static [SpecializedType]> {
    use SpecializedType::*;
    Some(match name {
        "Primitives" => &[Byte, Short, Int, Long, Char, Float, Double, Boolean, Unit],
        "Everything" => &[
            Byte, Short, Int, Long, Char, Float, Double, Boolean, Unit, AnyRef,
        ],
        "Bits32AndUp" => &[Int, Long, Float, Double],
        "Integral" => &[Byte, Short, Int, Long, Char],
        "AllNumeric" => &[Byte, Short, Int, Long, Char, Float, Double],
        "BestOfBreed" => &[Int, Double, Boolean, Unit, AnyRef],
        "Arg" => &[Int, Long, Float, Double],
        "Args" => &[Int, Long, Double],
        "Return" => &[Int, Long, Float, Double, Boolean, Unit],
        _ => return None,
    })
}

/// The last segment of `a.b.C`.
fn simple_name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// The arguments of `@Ann(a, b)`; empty for a bare `@Ann`.
fn annot_args(annot: &Tree) -> &[Tree] {
    match &annot.kind {
        TreeKind::Apply { args, .. } => args,
        _ => &[],
    }
}

/// The types `@specialized` selects, or `None` when `annot` is not a
/// `@specialized` at all.
///
/// The caller is responsible for having undone any import rename first;
/// [`canonical_specialization_name`] is how the parser does that.
pub fn specialized_types(annot: &Tree) -> Option<SpecializedTypes> {
    let path = annot.annotation_path();
    if simple_name(&path) != SPECIALIZED {
        return None;
    }
    let args = annot_args(annot);
    if args.is_empty() {
        // `@specialized T` and `@specialized() T` both reach nsc as an
        // annotation with no arguments, and both mean `Primitives`.
        return Some(SpecializedTypes::primitives());
    }
    let mut out = SpecializedTypes::EMPTY;
    for a in args {
        let n = simple_name(&a.annotation_path()).to_string();
        if let Some(t) = SpecializedType::from_name(&n) {
            out.insert(t);
        } else if let Some(g) = group(&n) {
            for t in g {
                out.insert(*t);
            }
        }
    }
    Some(out)
}

/// Whether `annot` is `@unspecialized`, which opts one member out of the
/// specialization its owner would otherwise give it.
pub fn is_unspecialized(annot: &Tree) -> bool {
    simple_name(&annot.annotation_path()) == UNSPECIALIZED
}

/// `specialized` / `unspecialized` if that is what this annotation path names,
/// and `None` otherwise. Only the last segment is looked at, so `@specialized`,
/// `@scala.specialized` and `@_root_.scala.specialized` are one answer.
pub fn canonical_specialization_name(path: &str) -> Option<&'static str> {
    match simple_name(path) {
        SPECIALIZED => Some(SPECIALIZED),
        UNSPECIALIZED => Some(UNSPECIALIZED),
        _ => None,
    }
}

/// Rewrite the head of an annotation tree to `name`.
///
/// `import scala.{specialized => sp}` makes `@sp` mean `@specialized`, and the
/// annotation tree records only the name the use site wrote. nsc resolves that
/// name to a symbol and every later phase sees `scala.specialized`; we have no
/// symbol on an annotation, so the parser normalises the spelling instead. That
/// is safe precisely because the annotation is inert: it is never type-checked,
/// never pickled (see `pickle_symannot`) and never emitted — the only thing
/// that reads it is the specialization record.
pub(crate) fn rename_head(annot: &mut Tree, name: &str) {
    match &mut annot.kind {
        TreeKind::Ident { name: n } => *n = name.to_string(),
        TreeKind::Select { name: n, .. } => *n = name.to_string(),
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => rename_head(fun, name),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::TreeKind;
    use crate::parse_str;

    /// The annotation on the first type parameter of the source's only class.
    fn tparam_annot(src: &str) -> Tree {
        let r = parse_str(src);
        assert!(
            !crate::has_errors(&r.diags),
            "parse errors: {:?}",
            r.diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let stats = match &r.tree.kind {
            TreeKind::PackageDef { stats, .. } => stats.clone(),
            _ => vec![r.tree.clone()],
        };
        for s in &stats {
            let TreeKind::ClassDef { tparams, .. } = &s.kind else {
                continue;
            };
            let Some(TreeKind::TypeDef { mods, .. }) = tparams.first().map(|t| &t.kind) else {
                continue;
            };
            if let Some(a) = mods.annotations.first() {
                return a.clone();
            }
        }
        panic!("no annotated type parameter in {src:?}");
    }

    fn selected(src: &str) -> Vec<&'static str> {
        specialized_types(&tparam_annot(src))
            .expect("a @specialized")
            .names()
    }

    #[test]
    fn bare_specialized_is_primitives() {
        assert_eq!(
            selected("class C[@specialized T]"),
            vec!["Byte", "Short", "Char", "Int", "Long", "Float", "Double", "Boolean", "Unit"]
        );
    }

    #[test]
    fn explicit_types() {
        assert_eq!(
            selected("class C[@specialized(Int, AnyRef) T]"),
            vec!["Int", "AnyRef"]
        );
        assert_eq!(selected("class C[@specialized(scala.Int) T]"), vec!["Int"]);
    }

    #[test]
    fn groups() {
        assert_eq!(
            selected("class C[@specialized(Specializable.Bits32AndUp) T]"),
            vec!["Int", "Long", "Float", "Double"]
        );
        // `import Specializable._` writes the group name bare.
        assert_eq!(
            selected("class C[@specialized(BestOfBreed) T]"),
            vec!["Int", "Double", "Boolean", "Unit", "AnyRef"]
        );
        assert_eq!(
            selected("class C[@specialized(Everything) T]").len(),
            SpecializedType::ALL.len()
        );
    }

    #[test]
    fn a_name_that_selects_nothing() {
        // nsc collects the argument's type symbol and then looks for it among
        // the value classes, so an unrelated name specializes at nothing.
        assert!(
            specialized_types(&tparam_annot("class C[@specialized(Foo) T]"))
                .expect("a @specialized")
                .is_empty()
        );
    }

    #[test]
    fn an_import_rename_is_undone_by_the_parser() {
        let src = "import scala.{specialized => sp}\nclass C[@sp(Long) T]";
        assert_eq!(tparam_annot(src).annotation_path(), "specialized");
        assert_eq!(selected(src), vec!["Long"]);
    }

    #[test]
    fn not_specialized_at_all() {
        assert!(specialized_types(&tparam_annot("class C[@deprecated T]")).is_none());
    }

    /// nsc's `-no-specialization` is *ignore the annotation*, so under it the
    /// tree does not keep it and nothing downstream can record it.
    #[test]
    fn no_specialization_drops_it() {
        let src = "import scala.{specialized => sp}\nclass C[@specialized(Int) T]\nclass D[@sp T]";
        let sf = scala_rs_span::SourceFile::new("test.scala", src);
        let opts = crate::ParseOptions {
            no_specialization: true,
            ..Default::default()
        };
        let r = crate::parse_file_opts(&sf, 0, opts);
        assert!(!crate::has_errors(&r.diags), "{:?}", r.diags);
        let TreeKind::PackageDef { stats, .. } = &r.tree.kind else {
            panic!("expected a compilation unit");
        };
        let classes: Vec<_> = stats
            .iter()
            .filter_map(|s| match &s.kind {
                TreeKind::ClassDef { tparams, .. } => Some(tparams.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(classes.len(), 2);
        for tparams in classes {
            let TreeKind::TypeDef { mods, .. } = &tparams[0].kind else {
                panic!("expected a type parameter");
            };
            assert!(mods.annotations.is_empty(), "{:?}", mods.annotations);
        }
    }
}
