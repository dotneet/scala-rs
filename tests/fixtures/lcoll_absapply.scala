// `underlying(j)` where `underlying`'s type is an abstract type whose bound
// declares `apply` -- `scala/collection/convert/impl/IndexedSeqStepper.scala`'s
// `IntIndexedSeqStepper[CC <: collection.IndexedSeqOps[Int, AnyConstr, _]]`.
object Main {
  type AnyC[X] = Any

  class Stepper[CC <: collection.IndexedSeqOps[Double, AnyC, _]](underlying: CC) {
    def at(j: Int): Double = underlying(j)
    def written(j: Int): Double = underlying.apply(j)
    def n: Int = underlying.length
  }

  // A plain collection bound, and one reached through a second parameter.
  class Plain[CC <: collection.IndexedSeq[Int]](xs: CC) {
    def at(j: Int): Int = xs(j)
  }

  def main(args: Array[String]): Unit = {
    val s = new Stepper[Vector[Double]](Vector(1.5, 2.5, 3.5))
    println(s.at(1))
    println(s.written(2))
    println(s.n)
    println(new Plain[Vector[Int]](Vector(10, 20)).at(1))
  }
}
