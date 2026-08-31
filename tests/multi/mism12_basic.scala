// A cake whose middle trait comes *later* on the command line than the leaf
// that extends it, and whose base the leaf's file never names: the leaf's
// inherited members are only visible after the second round of the header
// pass. This is slick's `BasicProfile` / `RelationalProfile` / `MemoryProfile`
// arrangement, reduced.
package mism12.basic

trait BasicProfile {
  /** Abstract in the base, fixed by the leaf. */
  type SchemaDescription <: SchemaDescriptionDef

  trait SchemaDescriptionDef {
    def show: String
  }
}
