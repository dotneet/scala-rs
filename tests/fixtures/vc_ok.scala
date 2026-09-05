// Value-class shapes SLS 5.1.7 / SIP-15 *allows*, and which the new
// restrictions in crates/typer/src/valueclass.rs must therefore keep
// accepting. Every line here is one scalac 2.13.16 compiles without a word
// (checked against /tmp/scala-2.13.16/bin/scalac); the four access modifiers
// on the parameter are `neg/valueclasses.scala`'s own "okay, wasn't allowed
// in 2.10.x" cases.
package vc

// The plain shape, plus methods -- a `def` in the body is not a field.
final class Meters(val n: Int) extends AnyVal {
  def double: Int = n * 2
  def plus(other: Meters): Int = n + other.n
}

// The four access modifiers a value class parameter may carry. `private` and
// `protected` still leave a getter, which is what the rule is really about;
// only the `[this]` forms do not.
class P1(private[vc] val x: Int) extends AnyVal
class P2(protected[vc] val x: Int) extends AnyVal
class P3(protected val x: Int) extends AnyVal
class P4(private val x: Int) extends AnyVal

// A `case class` parameter is a public `val` even though the source did not
// write one. cats writes exactly this shape
// (`final case class ShowInterpolator(_sc: StringContext) extends AnyVal`).
final case class Wrapped(u: Int) extends AnyVal

// Type parameters are fine; only `@specialized` ones are not.
final class Box[T](val t: T) extends AnyVal {
  def get: T = t
}

// A universal trait may be mixed in: the *first* parent is what nsc looks at.
trait Printable extends Any {
  def show: String
}
final class Tagged(val tag: String) extends AnyVal with Printable {
  def show: String = "#" + tag
}

// A member of an object is static, so it is neither "a member of another
// class" nor "a local class".
object Holder {
  final class Inner(val v: Int) extends AnyVal {
    def bump: Int = v + 1
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Meters(21).double)
    println(new Meters(1).plus(new Meters(2)))
    println(new Box[String]("boxed").get)
    println(new Tagged("t").show)
    println(new Holder.Inner(41).bump)
    println(new Wrapped(7).u)
  }
}
