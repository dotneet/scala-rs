// A case class field whose type is a value class is stored unboxed, but
// `productElement` hands out an *instance* -- nsc emits `new Meters(this.m())`
// in the switch arm, exactly as it does in `toString`.
object Main {
  class Meters(val n: Int) extends AnyVal {
    override def toString: String = "Meters(" + n + ")"
  }
  case class Box(m: Meters, b: String)
  case class Pair(a: Meters, c: Meters)

  def main(args: Array[String]): Unit = {
    val box = Box(new Meters(3), "b")
    println(box.productElement(0))
    println(box.productElement(1))
    println(box.productElementName(0))
    println(box.toString)
    val pair = Pair(new Meters(1), new Meters(2))
    println("" + pair.productElement(0) + " " + pair.productElement(1))
    println(pair.productArity)
  }
}
