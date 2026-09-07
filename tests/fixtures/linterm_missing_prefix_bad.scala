// A parent whose *qualifier* names nothing. Reduced from cats: with
// `Apply.scala` absent, every `trait AllOps extends Ops with Apply.AllOps` in
// the typeclass hierarchy resolved `Apply.AllOps` to the enclosing `AllOps` --
// the trait being defined -- because an unresolved qualified type falls back
// on the bare name (`check_types.rs`, the `TreeKind::Select` arm of
// `tree_to_type`). A missing file thereby became a self-inheriting trait, and
// a self-inheriting trait used to make the linearization walk run for hours.
//
// Real scalac 2.13.16 says, at line 18:
//
//   error: not found: value Missing
//
// This compiler said `illegal cyclic reference` until the fallback was
// narrowed to qualifiers that denote something (`qualifier_names_nothing`);
// it now says the same words on the same line. Keep this header 15 lines.
object Bar {
  trait Ops[A]
  trait AllOps[A] extends Ops[A] with Missing.AllOps[A]
}
