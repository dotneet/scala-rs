trait Box { type A; def x: A }
class StrBox extends Box { type A = String; def x: A = "hi" }
object Main {
  def get(b: Box { type A <: Int }): Int = b.x
  def main(args: Array[String]): Unit = println(get(new StrBox))
}
