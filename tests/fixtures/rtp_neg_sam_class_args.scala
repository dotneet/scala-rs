// scalac: an abstract class whose constructor takes arguments is not a SAM
// type, so the function literal does not convert (type mismatch).
object Main {
  abstract class H(x: Int) { def h(s: String): String }
  def main(args: Array[String]): Unit = { val h: H = (s: String) => s; println(h.h("x")) }
}
