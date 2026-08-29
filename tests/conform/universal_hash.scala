class C
object Main {
  def main(args: Array[String]): Unit = {
    val s = "a-b-c"
    println(s.replaceFirst("-", "+"))
    println(s.replaceAll("-", "+"))
    println(s.regionMatches(0, "a-b", 0, 3))
    println(s.regionMatches(true, 0, "A-B", 0, 3))
    val c = new C
    println(c.## == c.hashCode)
    println(1.##)
    println(1.0.##)
    println("x".##)
    println(3L.##)
  }
}
