// A chained package clause whose first segment is a package that already
// exists: `scala.runtime` used to be special-cased out of the binary name, so
// these three classes were written as `LfPkgCell.class` and friends in the
// default package. The standard library writes thirty-one of its own files this
// way (`package scala` / `package runtime`), so 143 of its classes -- including
// `scala.runtime.ScalaRunTime$` -- came out with a binary name no consumer can
// name and not the one the pickle claims.
package scala
package runtime

class LfPkgCell(val v: Int)

object LfPkgMain {
  def main(args: Array[String]): Unit = println(new LfPkgCell(42).v)
}
