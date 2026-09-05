// A case class's companion `unapply` has to exist as a real method.
//
// The pattern matcher reads the fields straight off the scrutinee, so nothing
// in the backend ever needed `Foo.unapply` -- and it was never emitted. Every
// program that named it itself died with
// `NoSuchMethodError: 'scala.Option Foo$.unapply(Foo)'`.
//
// A `case class ... extends AnyVal` erases to its single field's type, so its
// companion's `apply` is the identity and its `unapply` takes that erased type
// -- not the box. Emitting the boxed descriptors made every call site, which
// had been type-checked against the erased ones, miss.
case class Zero()
case class One(x: Int)
case class Two(s: String, n: Int)
case class Three(a: Int, b: Int, c: String)
case class Gen[T](t: T, u: T)
case class Wrap[T](t: T) extends AnyVal

// A companion the user wrote gets the same synthetic member.
case class Written(x: Int)
object Written {
  def twice(w: Written): Int = w.x * 2
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Zero.unapply(Zero()))
    println(One.unapply(One(7)))
    println(Two.unapply(Two("a", 2)))
    println(Three.unapply(Three(1, 2, "c")))
    println(Gen.unapply(Gen("l", "r")))
    // A value class's `apply` is the identity on the erased underlying type.
    println(Wrap.apply("w").t)
    println(Written.unapply(Written(4)))
    println(Written.twice(Written(4)))
    // Eta-expanded to a function value, which is what forces a real method.
    val f: One => Option[Int] = One.unapply
    println(f(One(9)))
    println(One.unapply(null))
  }
}
