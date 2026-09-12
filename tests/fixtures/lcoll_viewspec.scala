// Two views whose parameters differ only by an upper bound are ordered by it,
// the way `Predef`'s array wrappers are:
//
//   implicit def genericWrapArray[T](xs: Array[T]): ArraySeq[T]
//   implicit def wrapRefArray[T <: AnyRef](xs: Array[T]): ArraySeq.ofRef[T]
//   implicit def wrapCharArray(xs: Array[Char]): ArraySeq.ofChar
//
// `wrapRefArray` is strictly the more specific for an `Array[String]`, and is
// not a candidate at all for an `Array[Char]`.
object Main {
  class Box[T](val t: T)
  class Gen[T](val b: Box[T]) { def who: String = "gen" }
  class Refs[T <: AnyRef](val b: Box[T]) { def who: String = "ref" }
  class Chars(val b: Box[Char]) { def who: String = "char" }
  object P {
    implicit def gen[T](xs: Box[T]): Gen[T] = new Gen(xs)
    implicit def refs[T <: AnyRef](xs: Box[T]): Refs[T] = new Refs(xs)
    implicit def chars(xs: Box[Char]): Chars = new Chars(xs)
  }
  import P._

  def main(args: Array[String]): Unit = {
    println(new Box("s").who)
    println(new Box('c').who)
    println(new Box(1).who)
    // The real `Predef` wrappers, used as a conversion to an expected type.
    val a: Seq[String] = Array("p", "q")
    val c: Seq[Char] = Array('u', 'v')
    println(a.mkString(","))
    println(c.mkString("-"))
  }
}
