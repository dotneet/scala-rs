trait Named { def name: String }
class P(nm: String) extends Named { def name = nm }
object Main {
  def greet[A <: Named](x: A): String = "hi " + x.name
  def first[A <: P](x: A): String = x.name
  def main(args: Array[String]): Unit = {
    println(greet(new P("a")))
    println(first(new P("b")))
  }
}
