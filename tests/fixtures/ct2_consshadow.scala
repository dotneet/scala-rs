// A class that declares a method called `::` does not thereby hide the case
// class `scala.::` from the `case h :: t` patterns in its own body.
//
// nsc types a constructor pattern's function in `typingConstructorPattern`
// mode, where a non-stable method of that name does not qualify. cats'
// `NonEmptyList` is exactly this shape, and five of its patterns reported
// "not found: extractor ::".
//
// Written to the private runtime's subset (no `:::`, no `List(...)`, no
// `mkString`) so both runtimes can run it: the rule is name resolution, not
// anything the jar provides.
final case class NEL[+A](head: A, tail: List[A]) {
  def ::[AA >: A](a: AA): NEL[AA] = NEL(a, toList)

  def toList: List[A] = head :: tail

  def prependAll[AA >: A](other: List[AA]): NEL[AA] =
    other match {
      case Nil          => this
      case head :: tail => NEL(head, prependAll(tail).toList)
    }

  def render: String = {
    def go(xs: List[A]): String = xs match {
      case Nil     => "]"
      case h :: t  => "," + h.toString + go(t)
    }
    "[" + head.toString + go(tail)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(NEL(3, 4 :: Nil).prependAll(1 :: 2 :: Nil).render)
    println(NEL(3, 4 :: Nil).prependAll(Nil).render)
    println((0 :: NEL(1, 2 :: Nil)).render)
  }
}
