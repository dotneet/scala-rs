// Lambdas as arguments of List.apply with the expected type List[SAM]: the
// element prototype comes from the expected result type.
object Main {
  trait IntOp { def apply(x: Int): Int }
  def main(args: Array[String]): Unit = {
    val ops: List[IntOp] = List(_ + 10, x => x * x)
    println(ops.map(_(3)))
  }
}
