// The other side of the same rule: a generic parameter the subclass *fixes*
// still matches, so this really is an illegal override of a `final` member.
//
// `Cell[A]` read at `IntBox` is `Cell[Int]`, which is the parameter `put`
// declares -- the substitution succeeds and nothing about it is "certainly
// different". A matcher that decided overloads by shape alone, without reading
// the base at the overriding class, would let this through.
trait Cell[+A]
trait Box[A] {
  final def put(x: Cell[A]): Int = 1
}
class IntBox extends Box[Int] {
  def put(x: Cell[Int]): Int = 2
}
object Main {
  def main(args: Array[String]): Unit = println(new IntBox().put(null))
}
