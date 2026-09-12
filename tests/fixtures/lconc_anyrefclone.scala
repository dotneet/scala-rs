// `protected def clone(): AnyRef` is declared on `scala.AnyRef`
// (nsc's `src/library-aux/scala/AnyRef.scala`); `scala.Cloneable` is
// `java.lang.Cloneable`, a marker interface with no members of its own. So
// `collection/mutable/Cloneable.scala`'s `super.clone()` and every
// `this.clone()` in a `Cloneable` subclass reach that one declaration, and a
// protected member inherited from `AnyRef` itself has to be accessible in
// every subclass.
object Main {
  class Box(val n: Int) extends Cloneable {
    def copy1: Box = this.clone().asInstanceOf[Box]
    def copy2: Box = clone().asInstanceOf[Box]
    // Protected access is granted to a *sibling* instance of the accessing
    // class too, as nsc's `accessWithin` has it.
    def copyOf(other: Box): Box = other.clone().asInstanceOf[Box]
  }

  // An inaccessible universal member is no answer: the receiver's own public
  // override and an implicit view both come *after* it in nsc's order, and
  // both have to be reachable.
  class Foo(val bar: String)
  object Foo {
    implicit class Enrich(foo: Foo) {
      def clone(x: Int, y: Int): Int = x + y
    }
  }

  def main(args: Array[String]): Unit = {
    val b = new Box(7)
    println(b.copy1.n)
    println(b.copy2.n)
    println(b.copyOf(new Box(9)).n)
    println(b.copy1 ne b)
    // `scala.collection.mutable.Cloneable` declares a public
    // `override def clone(): C` over `AnyRef`'s protected one.
    val buf = scala.collection.mutable.ArrayBuffer(1, 2, 3)
    println(buf.clone())
    println(scala.collection.mutable.TreeSet(2, 1).clone())
    // `pos/t10206`: the view wins because `AnyRef.clone` is not accessible here.
    println(new Foo("hello").clone(1, 2))
  }
}
