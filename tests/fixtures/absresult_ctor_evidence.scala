trait CtorTC[T] { def text: String }
class BoundCtor[T: CtorTC](val i: Int, val j: Int) {
  def this(i: Int)(implicit j: Int) = this(i, j)
  def result: String = implicitly[CtorTC[T]].text + ":" + (i + j)
}
trait CtorNamed { def text: String }
class ViewedCtor[T <% CtorNamed](val value: T) {
  def this(value: T, ignored: Boolean) = this(value)
  def result: String = value.text
}
object PrivateCtor { private def initial: Int = 55 }
class PrivateCtor(val value: Int) { def this() = this(PrivateCtor.initial) }
object Main {
  def main(args: Array[String]): Unit = {
    implicit val tc: CtorTC[Int] = new CtorTC[Int] { def text: String = "int" }
    implicit val j: Int = 7
    println(new BoundCtor[Int](5).result)
    println(new PrivateCtor().value)
    implicit def named(i: Int): CtorNamed = new CtorNamed { def text: String = "view:" + i }
    println(new ViewedCtor[Int](3, true).result)
  }
}
