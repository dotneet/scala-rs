abstract class BaseFn[-A, +B] extends (A => B) {
  override def toString: String = "BaseFn$"
}

final case class Single[-A, +B](f: A => B) extends BaseFn[A, B] {
  def apply(a: A): B = f(a)
}

object Main {
  def main(args: Array[String]): Unit = {
    val single = Single((x: Int) => x)
    println(single.toString)
    println(single(7))
  }
}
