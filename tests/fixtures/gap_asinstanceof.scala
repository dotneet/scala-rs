class Base
class Derived extends Base

class Box[T](value: T) {
  def same(other: T): Boolean =
    value.asInstanceOf[AnyRef] eq other.asInstanceOf[AnyRef]
}

object Main {
  def foo(x: Any): String =
    if (x == null) null.asInstanceOf[String] else x.asInstanceOf[String]

  def main(args: Array[String]): Unit = {
    // `null.asInstanceOf[T]`: member resolution on `Type::Null`.
    println(foo("hi"))
    println(foo(null))

    // `x.asInstanceOf[T]` must actually type as `T`, not widen to `Any`
    // (a bare generic method-type-parameter substitution bug).
    val a: Any = "hi"
    val s: String = a.asInstanceOf[String]
    println(s.length)

    // isInstanceOf / asInstanceOf on primitives and classes.
    println(a.isInstanceOf[String])
    println(a.isInstanceOf[Int])
    val b: Any = 42
    println(b.isInstanceOf[Int])
    println(b.asInstanceOf[Int] + 1)
    val d: Base = new Derived
    println(d.isInstanceOf[Derived])

    // asInstanceOf on an unbounded type parameter (`Type::TypeParam`).
    val box = new Box("hi")
    println(box.same("hi"))
    println(box.same("other"))
  }
}
