// PartialFunction literals: isDefinedAt without evaluating the body,
// orElse/andThen/lift/applyOrElse, collect, and the MatchError of a
// non-exhaustive function literal.
object Main {
  var bodyRuns = 0
  val pf: PartialFunction[Int, String] = { case x if x > 0 => bodyRuns += 1; "pos" + x }
  val neg: PartialFunction[Int, String] = { case x if x < 0 => "neg" + x }
  val zero: PartialFunction[Int, String] = { case 0 => "zero" }
  def main(args: Array[String]): Unit = {
    println(pf.isDefinedAt(5) + " " + pf.isDefinedAt(-5) + " runs=" + bodyRuns)
    val all = pf orElse neg orElse zero
    println(List(-2, 0, 3).map(all))
    println(pf.lift(1) + " " + pf.lift(-1) + " " + pf.applyOrElse(-7, (x: Int) => "default" + x))
    println((pf andThen (_.length)).lift(12345))
    println(List(1, -1, 2, 0).collect(pf) + " " + List(1, -1, 2, 0).collect(all))
    val f: Int => String = { case 1 => "one"; case 2 => "two" }
    println(f(1))
    try println(f(3)) catch { case e: MatchError => println("MatchError " + e.getMessage) }
    val tupled: PartialFunction[(Int, String), String] = { case (n, s) if n > 0 => s * n }
    println(List((2, "ab"), (0, "zz"), (1, "c")).collect(tupled))
    val anyPf: PartialFunction[Any, String] = { case s: String => "s:" + s; case i: Int => "i:" + i }
    println(List[Any]("x", 1, 2.0).collect(anyPf))
    println(Map(1 -> "a", 2 -> "b").collect { case (k, v) if k > 1 => v * 2 })
    val cond = PartialFunction.cond(5) { case x if x > 3 => true }
    val condOpt = PartialFunction.condOpt("hi") { case "hi" => 1 }
    println(cond + " " + condOpt)
    println(bodyRuns)
    val counted = scala.collection.mutable.ListBuffer.empty[Int]
    val guarded: PartialFunction[Int, Int] = { case x if { counted += x; x % 2 == 0 } => x }
    println(List(1, 2, 3, 4).collect(guarded) + " guard-evals=" + counted)
    val composed = pf.orElse[Int, String] { case _ => "fallback" }
    println(composed(-100))
    val fromFn = PartialFunction.fromFunction((x: Int) => x + 1)
    println(fromFn.isDefinedAt(99) + " " + fromFn(1))
  }
}
