object Main {
  def main(args: Array[String]): Unit = {
    val s = "  Hello World  "
    println(s.trim)
    println("abcdef".substring(2))
    println("abcdef".substring(1, 3))
    println("abcabc".indexOf("b"))
    println("abcabc".lastIndexOf("b"))
    println("abc".replace('a', 'z'))
    println("hello world".replace("world", "there"))
    println("abc".contains("b"))
    println("abc".contains("z"))
    println("ABC".equalsIgnoreCase("abc"))
    println("abc123".matches("[a-z]+[0-9]+"))
    println("abc".concat("def"))
    println("  x  ".strip)
    println("ab".repeat(3))
    println("abc".compareTo("abd"))
  }
}
