// A second compilation unit of the same package. What its package clause
// makes available in `Main_1.scala` is SLS 2 precedence *4*, below a wildcard
// import -- and it is nsc's documented exception to the ambiguity this slice
// implements (`isPackageOwnedInDifferentUnit`): there the import simply wins.
package nameamb

object Elsewhere {
  def who: String = "sibling-unit"
}
