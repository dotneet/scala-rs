// `eq` and `ne` on a class whose only ancestors are *universal traits*.
//
// `lookup_member` finds `AnyRef`'s members by walking a declared parent, and
// `rough_parents` supplies `AnyRef` only when the parent list is empty. A
// class that mixes in a trait extending `Any` therefore had a chain that
// ended at `Any`, and `t eq null` was `value eq is not a member of T2[A, B]`
// -- while the very same program's `def conforms(t: T2[Int, Int]): AnyRef = t`
// was accepted. That is `src/library`'s whole `Tuple`/`Product`/`Iterator`
// family: `scala.Equals` is `trait Equals extends scala.Any`.
//
// Run rather than compiled, because `eq` is reference identity: a fallback
// that resolved it to `==` would print the same for `p` and `q` here and
// still type-check.
trait Eqls extends Any {
  def canEqual(that: Any): Boolean
}
trait Prod extends Any with Eqls {
  def productArity: Int
}
final class T2[A, B](val _1: A, val _2: B) extends Prod {
  def productArity: Int = 2
  def canEqual(that: Any): Boolean = that.isInstanceOf[T2[_, _]]
  override def equals(that: Any): Boolean = that match {
    case o: T2[_, _] => _1 == o._1 && _2 == o._2
    case _ => false
  }
  override def hashCode: Int = 0
  override def toString: String = "T2(" + _1 + "," + _2 + ")"
}

object Main {
  def conforms(t: T2[Int, Int]): AnyRef = t

  def main(args: Array[String]): Unit = {
    val p = new T2(1, 2)
    val q = new T2(1, 2)
    val n: T2[Int, Int] = null
    // Same value, different objects: `eq` is false where `==` is true.
    println("p eq q: " + (p eq q))
    println("p == q: " + (p == q))
    println("p eq p: " + (p eq p))
    println("p ne q: " + (p ne q))
    println("n eq null: " + (n eq null))
    println("p ne null: " + (p ne null))
    println("conforms: " + conforms(p))
  }
}
