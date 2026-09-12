// The valid neighbours of wacc_flatmap_bad and wacc_newtargs_bad: a `flatMap`
// body that is an `IterableOnce` one way or another (a collection, a `String`
// through `StringOps`, an `Option` through `option2Iterable`, an `Array`
// through a wrapper), and `new` with its arguments supplied.
class Gen[T](val a: T)
class Pair[A, B](val a: A, val b: B)
object Main {
  def main(args: Array[String]): Unit = {
    println(Seq(1, 2).flatMap(x => Seq(x, x)))
    println(Seq("ab").flatMap(x => x))
    println(Seq(1).flatMap(x => Some(x)))
    println(Seq(1, 2).flatMap(x => Array(x, x).toSeq))
    println(Option(1).flatMap(x => Option(x + 1)))
    println(Map(1 -> "a").flatMap { case (k, v) => List((k, v)) })
    println(List(1).flatMap(x => List(x, x)))
    println(Iterator(1, 2).flatMap(x => Iterator(x)).toList)
    println(Seq(1).flatMap(x => if (x > 0) Some(x) else None))
    println(new Gen[Int](3).a + " " + new Pair[Int, String](1, "s").b)
    // The only constructor clause is an implicit `ClassTag`, which the call
    // site supplies: `new` with no argument list is right here.
    val ub = new scala.collection.mutable.UnrolledBuffer[Int]
    ub += 7
    println(ub.size + " " + ub.head)
  }
}
