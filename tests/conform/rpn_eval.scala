object Main {
  abstract class Op { def apply(a: Double, b: Double): Double; val sym: String }
  val ops: Map[String, Op] = Map(
    "+" -> new Op { def apply(a: Double, b: Double) = a + b; val sym = "+" },
    "*" -> new Op { def apply(a: Double, b: Double) = a * b; val sym = "*" })
  def eval(tokens: List[String]): Double = {
    val st = scala.collection.mutable.Stack[Double]()
    tokens.foreach {
      case t if ops.contains(t) => val b = st.pop(); val a = st.pop(); st.push(ops(t)(a, b))
      case n => st.push(n.toDouble)
    }
    st.pop()
  }
  def main(a: Array[String]): Unit = {
    println(eval("3 4 + 2 *".split(" ").toList))
    println(eval(List("10", "2", "+")))
  }
}
