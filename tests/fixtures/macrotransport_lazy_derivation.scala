import shapeless._

trait Show[A] { def show(a: A): String }
object Show {
  implicit val int: Show[Int] = (a: Int) => a.toString
  implicit val str: Show[String] = (a: String) => a
  implicit val hnil: Show[HNil] = (_: HNil) => ""
  implicit def hcons[H, T <: HList](implicit h: Lazy[Show[H]], t: Show[T]): Show[H :: T] =
    (a: H :: T) => h.value.show(a.head) + "," + t.show(a.tail)
  implicit def generic[A, R](implicit gen: Generic.Aux[A, R], r: Lazy[Show[R]]): Show[A] =
    (a: A) => "(" + r.value.show(gen.to(a)) + ")"
}

case class Leaf(a: Int, b: String)
case class Mid(x: Int, leaf: Leaf, y: String)
case class Top(mid: Mid, leaf: Leaf, z: Int)

object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[Show[Leaf]].show(Leaf(1, "a")))
    println(implicitly[Show[Mid]].show(Mid(2, Leaf(3, "b"), "c")))
    println(implicitly[Show[Top]].show(Top(Mid(4, Leaf(5, "d"), "e"), Leaf(6, "f"), 7)))
    // A nested derivation spells every path from `_root_` and asks
    // `c.openImplicits` at each level; the engine is sent each open implicit
    // once and each source file once.
    println(implicitly[Show[Int :: Leaf :: HNil]].show(8 :: Leaf(9, "g") :: HNil))
  }
}
