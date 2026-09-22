trait Base[A] {
  def content: A
}

trait Flag

object PatternProbe {
  def pure[A](b: Base[A]): A = b match {
    case x: Flag => x.content
    case _ => b.content
  }

  def bound[A](b: Base[A]): A = b match {
    case x @ (_: Flag) => x.content
    case _ => b.content
  }

  def passed[A](b: Base[A]): A = b match {
    case x: Flag => take(x)
    case _ => b.content
  }

  def take[A](x: Base[A] with Flag): A = x.content
}

final class Both(val content: Int) extends Base[Int] with Flag

object Main {
  def main(args: Array[String]): Unit = {
    val b: Base[Int] = new Both(5)
    println(PatternProbe.pure(b))
    println(PatternProbe.bound(b))
    println(PatternProbe.passed(b))
  }
}
