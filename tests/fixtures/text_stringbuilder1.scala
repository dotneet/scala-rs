object Main {
  def main(args: Array[String]): Unit = {
    val sb = new StringBuilder()
    sb.append("hello")
    sb.append(' ')
    sb.append(42)
    sb += '!'
    println(sb.toString)
    println(sb.length)
    println(sb.isEmpty)
    sb.insert(0, ">>")
    println(sb.toString)
    sb.deleteCharAt(0)
    println(sb.toString)
    sb.setLength(3)
    println(sb.toString)
    println(sb.reverse().toString)
    sb.clear()
    println(sb.isEmpty)
    val sb2 = new StringBuilder()
    sb2 ++= "abc"
    println(sb2.result())
    println(sb2.nonEmpty)
    println(sb2.charAt(1))
  }
}
