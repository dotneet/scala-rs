class Holder {
  var s: String = _
  var i: Int = _
  var b: Boolean = _
  var d: Double = _
  def show: String = s + "|" + i + "|" + b + "|" + d
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Holder().show)
    val f: Int => Int = _ + 1
    println(f(1))
  }
}
