package cpvc

// Compiled by real scalac and handed to scala-rs on `-cp`. The point is the
// *class file* shape: `Meters` erases to `int`, its methods live on `Meters$`
// as `$extension` methods, and `Factory.make` really has the descriptor `(I)I`.
// `extends AnyVal` survives only in the ScalaSignature -- the class file says
// `extends java/lang/Object` -- which is what made every value class arriving
// from `-cp` look like an ordinary class to scala-rs.
//
// Two of these are commented out on purpose; see `cpvalueclass_use.scala`.
class Meters(val n: Int) extends AnyVal {
  def describe: String = "Meters(" + n + ")"
  def plus(m: Int): Meters = new Meters(n + m)
  def raw: Int = n
}

// A value class over a reference type, to check that the underlying is not
// assumed primitive.
class Name(val s: String) extends AnyVal {
  def shout: String = s.toUpperCase
}

trait Described extends Any { def label: String }

// A value class that also extends a universal trait: the class file *does*
// have an interface, so the `AnyVal` parent has to be added rather than
// substituted.
class Tagged(val t: String) extends AnyVal with Described {
  def label: String = "#" + t
}

// The fs2 shape exactly: a value class nested in an object, reached through a
// method whose descriptor is the underlying type. Nothing nested gets nsc's
// static forwarders, so the only `$extension` in the class files is the
// instance method on the companion module.
object Holder {
  class Partial(val strict: Boolean) extends AnyVal {
    def apply[A](it: Iterator[A], chunkSize: Int): String =
      s"strict=$strict chunk=$chunkSize items=${it.mkString(",")}"
  }
  def partial(strict: Boolean): Partial = new Partial(strict)
}

// A method whose *result* is a value class, so its descriptor is the
// underlying type and every call on the result is an `$extension`.
object Factory {
  def make(n: Int): Meters = new Meters(n)
  def named(s: String): Name = new Name(s)
  def tagged(s: String): Tagged = new Tagged(s)
  def firstOf(ms: List[Meters]): Meters = ms.head
}
