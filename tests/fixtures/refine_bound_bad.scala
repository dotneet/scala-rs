trait Box { type A; def x: A }
class StrBox extends Box { type A = String; def x: A = "hi" }
object Main {
  def main(args: Array[String]): Unit = {
    val b: Box { type A <: Int } = new StrBox
    println(b.x)
  }
}
