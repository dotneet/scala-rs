trait Shape[A, B] { def apply(a: A): B }
object Shape { implicit val intString: Shape[Int, String] = new Shape[Int, String] { def apply(a: Int): String = "value=" + a } }
class Proven[B](val value: B)
object Proven { implicit def prove[A, B](a: A)(implicit shape: Shape[A, B]): Proven[B] = new Proven(shape(a)) }
object Main { def main(args: Array[String]): Unit = { val p: Proven[String] = 7; println(p.value) } }
