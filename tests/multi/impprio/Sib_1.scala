// A second compilation unit of package `p5`. What it defines is "made
// available by a package clause" for `Main_1.scala` as well -- but from
// *another* unit, which is SLS 2 precedence 4 and therefore ranks BELOW a
// wildcard import. Getting this wrong is as much a wrong program as getting
// the same-unit case wrong, in the other direction.
package p5

object Sibling { def who: String = "package-elsewhere" }
