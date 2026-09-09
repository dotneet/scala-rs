class Base extends (Int => Any) { def apply(i: Int): Any = i + 1 }
class Child extends Base
object Main { def main(args: Array[String]): Unit = {
  val f: Int => Any = new Child
  println(f(41))
}}
