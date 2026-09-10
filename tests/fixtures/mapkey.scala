object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> "one")
    println(m("a"))
    println(m.get("a"))
    println(m.contains("a"))
    println(m.getOrElse("missing", "fallback"))
    println(m.updated("b", 2)("b"))
    println(m.getOrElse("missing", 3))
    val ints = Map(1 -> "int")
    println(ints(1))
    val any: Map[Any, String] = Map[Any, String](1 -> "wide")
    println(any(1))
    implicit def intKey(i: Int): String = "a"
    println(m(1))
    println(m.get(1))
    println(m.contains(1))
    println(m.getOrElse(1, "fallback"))
    println(m.updated(1, "changed")("a"))
  }
}
