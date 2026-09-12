// FunctionK / `~>`, including `FunctionK.lift`, whose implementation is cats'
// one macro (`cats/arrow/FunctionKMacros.scala`, matched with quasiquote
// *patterns*). If our expansion of that macro is wrong, this is where it shows.
import cats._
import cats.arrow.FunctionK
import cats.data._
import cats.syntax.all._

object Main {
  val listToOption: List ~> Option = new (List ~> Option) {
    def apply[A](fa: List[A]): Option[A] = fa.headOption
  }

  // `FunctionK.lift` takes a *polymorphic method reference*, not a function
  // literal: its macro matches `q"($param) => $trans[..$typeArgs]($arg)"`.
  def firstOption[A](l: List[A]): Option[A] = l.headOption
  def lastOption[A](l: List[A]): Option[A] = l.lastOption
  def toList[A](o: Option[A]): List[A] = o.toList

  def main(args: Array[String]): Unit = {
    println(listToOption(List(1, 2, 3)))
    println(listToOption(Nil: List[Int]))
    println(listToOption(List("a")))

    // FunctionK.lift -- the macro
    val fk: List ~> Option = FunctionK.lift[List, Option](firstOption)
    println(fk(List(9, 8)))
    println(fk(Nil: List[Int]))
    println(fk(List("z", "y")))

    val fk2: Option ~> List = FunctionK.lift[Option, List](toList)
    println(fk2(Option(5)))
    println(fk2(None: Option[Int]))

    // id, compose, andThen, or
    val idk: List ~> List = FunctionK.id[List]
    println(idk(List(1, 2)))
    println((listToOption.compose(fk2))(Option(4)))
    println((fk2.andThen(listToOption))(Option(6)))

    val both: EitherK[List, Option, *] ~> Option = listToOption.or(FunctionK.id[Option])
    println(both(EitherK.leftc[List, Option, Int](List(1, 2))))
    println(both(EitherK.rightc[List, Option, Int](Option(3))))

    // and, with a Tuple2K target
    val andFk = fk.and(FunctionK.lift[List, Option](lastOption))
    println(andFk(List(1, 2, 3)).first)
    println(andFk(List(1, 2, 3)).second)

    // natural transformations through a structure
    println(OptionT(List(Option(1), None)).mapK(listToOption).value)
    println(EitherT(List(Right(1): Either[String, Int])).mapK(listToOption).value)
    println(Nested(List(Option(1), Option(2))).mapK(listToOption).value)

    // FunctionK as a value in a data structure
    val fks: List[List ~> Option] = List(listToOption, fk, idk.andThen(listToOption))
    println(fks.map(f => f(List(7, 6))))

    // Representable (cheap case: Function1 from a fixed domain)
    val rep = Representable[Function1[Boolean, *]]
    println(rep.index(rep.tabulate[Int](b => if (b) 1 else 0))(true))
    println(rep.index(rep.tabulate[Int](b => if (b) 1 else 0))(false))
  }
}
