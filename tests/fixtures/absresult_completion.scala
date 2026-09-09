trait TC[A] { def value: String }
class TCImpl[A](val value: String) extends TC[A]
class Scope {
  implicit def tc = new TCImpl[Int]("completed")
  trait Need { def run: String = implicitly[TC[Int]].value }
  def result: String = new Need {}.run
}
object Main { def main(args: Array[String]): Unit = println(new Scope().result) }
