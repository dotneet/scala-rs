trait Show[A] { def show(a: A): String }
object Main {
  def make(prefix: String): Show[Option[Int]] = new Show[Option[Int]] {
    def show(a: Option[Int]): String = a match {
      case Some(v) => prefix + v
      case None    => prefix + "none"
    }
  }
  def guarded(limit: Int): Show[Int] = new Show[Int] {
    def show(a: Int): String = a match {
      case n if n > limit => "big " + n
      case n              => "small " + n
    }
  }
  def caught(tag: String): Show[Int] = new Show[Int] {
    def show(a: Int): String =
      try { if (a == 0) throw new RuntimeException("zero") else "ok " + a }
      catch { case e: RuntimeException => tag + e.getMessage }
  }
  def main(args: Array[String]): Unit = {
    println(make("p:").show(Some(3)))
    println(make("p:").show(None))
    println(guarded(5).show(9))
    println(guarded(5).show(1))
    println(caught("t:").show(0))
    println(caught("t:").show(4))
  }
}
