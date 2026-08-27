object Main {
  def main(args: Array[String]): Unit = {
    val xs = new java.util.ArrayList[String]()
    xs.add("hi")
    println(xs.get(0))
    println(xs.get(0).length)
    val e = new java.util.AbstractMap.SimpleEntry[String, String]("k", "v")
    println(e.getKey)
    println(e.getValue)
    val asEntry: java.util.Map.Entry[String, String] = e
    println(asEntry.getKey)
    println(java.lang.String.format("%s-%d", "x", 3))
    println(java.util.Arrays.asList("a", "b").size())
  }
}
