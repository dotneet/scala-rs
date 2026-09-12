// Traverse.traverse / sequence / Foldable.foldMap -- the part of cats that
// actually walks a structure, with the Applicative doing the accumulating.
import cats._
import cats.data._
import cats.syntax.all._

object Main {
  def parse(s: String): Option[Int] =
    if (s.forall(_.isDigit) && s.nonEmpty) Some(s.toInt) else None

  def main(args: Array[String]): Unit = {
    println(Traverse[List].traverse(List("1", "2", "3"))(parse))
    println(Traverse[List].traverse(List("1", "x", "3"))(parse))
    println(Traverse[List].sequence(List(Option(1), Option(2))))
    println(Traverse[List].sequence(List(Option(1), None)))
    println(Traverse[Option].traverse(Option("4"))(parse))
    println(Traverse[Option].traverse(None: Option[String])(parse))

    type E[A] = Either[String, A]
    def parseE(s: String): E[Int] = parse(s).toRight("bad:" + s)
    println(Traverse[List].traverse(List("1", "2"))(parseE))
    println(Traverse[List].traverse(List("1", "q", "z"))(parseE))

    // syntax
    println(List("1", "2").traverse(parse))
    println(List(Option(1), Option(2)).sequence)
    println(List("1", "2", "3").traverse_(parse))
    println(List(1, 2, 3).traverse(i => Option(i * i)))

    // Foldable
    println(Foldable[List].foldMap(List(1, 2, 3))(_.toString))
    println(Foldable[List].foldMap(List(1, 2, 3))(identity))
    println(Foldable[List].fold(List("a", "b", "c")))
    println(Foldable[List].foldLeft(List(1, 2, 3), 0)(_ + _))
    println(Foldable[List].foldRight(List(1, 2, 3), Eval.now(0))((a, b) => b.map(_ + a)).value)
    println(Foldable[Option].foldMap(Option(2))(_.toString))
    println(Foldable[List].size(List(1, 2, 3)))
    println(Foldable[List].isEmpty(Nil: List[Int]))
    println(Foldable[List].find(List(1, 2, 3))(_ > 1))
    println(Foldable[List].exists(List(1, 2, 3))(_ > 2))
    println(Foldable[List].forall(List(1, 2, 3))(_ > 0))
    println(List(1, 2, 3).foldMap(i => List(i, i)))
    println(List("a", "b").combineAll)
    println(List(1, 2, 3).foldMapA(i => Option(List(i))))

    // Functor composition through Nested
    val n: Nested[List, Option, Int] = Nested(List(Option(1), None, Option(3)))
    println(n.map(_ + 1).value)
    println(Functor[Nested[List, Option, *]].map(n)(_ * 2).value)
  }
}
