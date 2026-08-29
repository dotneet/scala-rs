object Main {
  implicit class RichInt(val n: Int) extends AnyVal { def twice: Int = n * 2 }
  class Meters(val m: Int) extends AnyVal { def plus(o: Meters): Meters = new Meters(m + o.m) }
  def main(args: Array[String]): Unit = {
    println(3.twice)
    println(new Meters(2).plus(new Meters(5)).m)
    val any: Any = new Meters(4)
    println(any)
  }
}
