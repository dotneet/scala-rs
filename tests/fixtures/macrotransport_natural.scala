import shapeless.nat.{_0, _1, _2, _10}
import shapeless.ops.nat.ToInt
object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[ToInt[_0]].apply())
    println(implicitly[ToInt[_1]].apply())
    println(implicitly[ToInt[_2]].apply())
    println(implicitly[ToInt[_10]].apply())
  }
}
