// The client half of the pickle round trip: always compiled by **real scalac**,
// against a `rh_pickle_lib` built either by scala-rs or by scalac. See
// `crates/cli/tests/rh.rs`; the two must print the same thing.
import rhpickle._
import rhpickle.all._

object Main {
  def firstOption[A](l: List[A]): Option[A] = l.headOption

  def main(args: Array[String]): Unit = {
    // a type lambda in an implicit's subject position
    val f = implicitly[Fun[Val2[String, *]]]
    println(f.fmap(Good(2): Val2[String, Int])(_ + 1))
    println(f.fmap(Bad("e"): Val2[String, Int])(_ + 1))
    println(Val2.funForVal2[Int].fmap(Good("s"): Val2[Int, String])(_ + "!"))

    // a case class's synthetic apply, copy and unapply
    println(Good(2))
    println(Good("s"))
    println(Pair(1, "a"))
    println(Pair(1, "a").copy(b = "b"))
    Good(5) match { case Good(x) => println(x + 1) }

    // a value class's extension method, with three parameter clauses
    println(List(1, 2).map2(List(10, 20))(_ + _))
    println(new ApOps(List(1)).tag)

    // an existential function parameter, fed a polymorphic method
    val fk: FK[List, Option] = FK.lift[List, Option](firstOption)
    println(fk(List(9, 8)))
    println(fk(Nil: List[Int]))
  }
}
