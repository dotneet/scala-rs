import scala.util.control.NonFatal
object First { def unapply(s: String): Option[Char] = s.headOption }
object Pair { def unapply(s: String): Option[(String, String)] = Some((s.take(1), s.drop(1))) }
object Main {
  def main(args: Array[String]): Unit = {
    try { throw new IllegalStateException("boom") }
    catch { case NonFatal(e) => println(e.getMessage + " / " + e.getClass.getName) }
    "scala" match { case First(c) => println(c) }
    "scala" match { case Pair(h, t) => println(h + "|" + t.length) }
  }
}
