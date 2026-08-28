object Main {
  def main(args: Array[String]): Unit = {
    val s = "Hello, World"
    println(s.length)
    println(s.toUpperCase)
    println(s.substring(0, 5))
    println(s.indexOf("World"))
    println(s.replace("World", "Scala"))
    println(s.split(", ").length)
    println(s.contains("lo,"))
    println(s.startsWith("He"))
    println(s.trim.isEmpty)
    println(s"len=${s.length} up=${s.take(3)}")
    println("a" * 3)
    val sb = new StringBuilder
    sb.append("a").append(1).append(true)
    println(sb.toString)
  }
}
