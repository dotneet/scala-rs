// Separate compilation: the three shapes whose classfiles scala-rs used not
// to write at all. Compiled on its own; `mcls_main.scala` is compiled against
// the *classfiles* this produces, by scala-rs and (when it is installed) by
// real scalac.
//
//   * a package object     -> mcls/util/package$.class AND mcls/util/package.class
//   * a value class        -> mcls/Meters$.class holding its `$extension` methods
//   * an operator-named    -> mcls/Codes$$colon$at$.class, not `Codes$:@$.class`
//     nested object
package mcls

package object util {
  val greeting: String = "hi"
  def twice(n: Int): Int = n * 2
}

class Meters(val v: Int) extends AnyVal {
  def plus(o: Int): Int = v + o
}

class Wrap[A](val a: A) extends AnyVal {
  def self: A = a
}

object Codes {
  object :@ {
    def id(n: Int): Int = n
  }
}
