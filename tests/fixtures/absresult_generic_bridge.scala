trait Wide[A] { def render(a: A): Any }
class Provider[A] { def render(a: A): String = a.toString }
class Child extends Provider[Int] with Wide[Int]
object Main { def main(args: Array[String]): Unit = {
  val w: Wide[Int] = new Child
  println(w.render(42))
}}
