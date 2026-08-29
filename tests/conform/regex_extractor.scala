object Main {
  val Num = "^(-?[0-9]+)$".r
  val Pair = "([a-z]+)=([0-9]+)".r
  def classify(s: String): String = s match {
    case Num(v)      => "num " + v
    case Pair(k, v)  => "pair " + k + ":" + v
    case _           => "other"
  }
  def main(args: Array[String]): Unit = {
    println(classify("-42"))
    println(classify("x=7"))
    println(classify("??"))
  }
}
