// scalac: type mismatch -- an abstract class whose constructor takes
// arguments is not a SAM type, so a function literal does not convert to it.
object Main {
  abstract class H(x: Int) { def h(s: String): String }
  def main(args: Array[String]): Unit = { val h: H = s => s; println(h.h("x")) }
}
