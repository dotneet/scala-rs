object Main {
  def main(args: Array[String]): Unit = {
    val c: Class[_] = java.lang.Class.forName("java.lang.String")
    println(c.getName)
    val xs = new java.util.ArrayList[java.lang.Byte]()
    xs.add(java.lang.Byte.valueOf("1"))
    xs.add(java.lang.Byte.valueOf("9"))
    val ys: java.util.Collection[_ <: java.lang.Number] = java.util.Collections.unmodifiableList(xs)
    println(ys.size())
    val m: java.lang.Byte = java.util.Collections.max(xs)
    println(m.intValue())
  }
}
