class FS(val n: String) {
  def unapply(s: String): Option[String] = if (s.startsWith(n)) Some(s.drop(n.length)) else None
}
object Lib { val Cast = new FS("c:") }
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List("c:one", "two", "c:three")
    xs.foreach {
      case Lib.Cast(rest) => println("cast " + rest)
      case other          => println("plain " + other)
    }
  }
}
