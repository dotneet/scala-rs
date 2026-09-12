// A tuple binder read out of an erased field keeps its *own* arity. The cast
// after the extraction was hard-coded to `scala/Tuple2`, so
// `{ case (inner, id) => … }` on a `((A, B, C), Long)` cast the triple to a pair:
// `ClassCastException: scala.Tuple3 cannot be cast to scala.Tuple2`. gitbucket's
// `Repositories.*` destructures a `(Tuple12, Tuple8, Long)`, so every row read
// back through it threw.
object Main {
  val t2: (((Int, Int), Long)) => String = { case (a, c) => a._2.toString + c }
  val t3: (((Int, Int, Int), Long)) => String = { case (a, c) => a._3.toString + c }
  val t4: (((Int, Int, Int, Int), Long)) => String = { case (a, c) => a._4.toString + c }
  val t12: (((Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int), Long)) => String = {
    case (a, c) => a._12.toString + c
  }
  val n3: (((Int, Int, Int), (String, String), Long)) => String = { case (a, b, c) =>
    a._3.toString + b._2 + c
  }
  def pick(p: (Option[(Int, Int, Int)], String)): String = p match {
    case (Some(t), s) => t._2.toString + s
    case (None, s)    => "none" + s
  }

  def main(args: Array[String]): Unit = {
    println(t2(((1, 2), 9L)))
    println(t3(((1, 2, 3), 9L)))
    println(t4(((1, 2, 3, 4), 9L)))
    println(t12(((1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12), 9L)))
    println(n3(((1, 2, 3), ("x", "y"), 9L)))
    println(pick((Some((7, 8, 9)), "!")))
    println(pick((None, "?")))
  }
}
