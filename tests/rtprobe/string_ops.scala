// StringOps and java.lang.String methods: slicing, splitting (regex and
// limits), padding, stripMargin, character tests, conversions, and the
// Seq view of a String.
object Main {
  def main(args: Array[String]): Unit = {
    val s = "Hello, World"
    println(s.length + " " + s.toUpperCase + " " + s.toLowerCase + " " + s.reverse + " " + s.take(5) + " " + s.drop(7) + " " + s.takeRight(3) + " " + s.dropRight(7))
    println(s.substring(7) + " " + s.substring(0, 5) + " " + s.slice(2, 4) + " " + s.charAt(4) + " " + s(0) + " " + s.indexOf('o') + " " + s.lastIndexOf("o") + " " + s.indexOf("zz"))
    println(s.split(", ").toList + " " + "a,b,,c,,".split(",").toList + " " + "a,b,,c,,".split(",", -1).toList + " " + "a1b22c".split("\\d+").toList + " " + "x.y".split('.').toList)
    println(s.contains("World") + " " + s.startsWith("Hell") + " " + s.endsWith("d") + " " + s.isEmpty + " " + "".nonEmpty + " " + s.matches(".*W.*"))
    println(s.replace('l', 'L') + " " + s.replace("World", "There") + " " + s.replaceAll("[aeiou]", "*") + " " + s.replaceFirst("l+", "_"))
    println("  pad  ".trim + "|" + "  pad  ".strip + "|" + "x".padTo(4, '.') + "|" + "ab" * 3 + "|" + "abc".map(_.toUpper) + "|" + "abc".filter(_ != 'b'))
    println("""|line1
               |line2""".stripMargin + " " + "a-b".stripPrefix("a-") + " " + "file.scala".stripSuffix(".scala"))
    println("42".toInt + 1 + " " + "3.5".toDouble * 2 + " " + "true".toBoolean + " " + "99".toLong + " " + "x".toIntOption + " " + "12".toIntOption + " " + "7f".toByteOption)
    println("hello".count(_ == 'l') + " " + "hello".distinct + " " + "hello".sorted + " " + "hello".groupBy(identity).size + " " + "hello".toSet.size + " " + "hello".head + " " + "hello".last)
    println("abc".zip("xyz").map { case (a, b) => s"$a$b" }.mkString(",") + " " + "abc".zipWithIndex.toList + " " + "a b  c".split(" +").length)
    println("Scala".compareTo("Scalb") + " " + "a".compareToIgnoreCase("A") + " " + "abc".equalsIgnoreCase("ABC") + " " + ("abc" < "abd") + " " + List("b", "a").max)
    println("hello world foo".split(" ").map(_.capitalize).mkString(" ") + " " + "CamelCaseString".filter(_.isUpper) + " " + "a1b2".partition(_.isDigit))
    println("%d-%s".format(1, "x") + " " + "tab\there".length + " " + "é".length + " " + "日本".length + " " + "A")
    println("abc".toList + " " + "abc".toArray.length + " " + "abc".iterator.map(_.toInt).sum + " " + "abc".foldLeft(0)(_ + _) + " " + "abc".sum)
    println("a,b;c".split("[,;]").mkString("|") + " " + "  lead".dropWhile(_ == ' ') + " " + "trail  ".reverse.dropWhile(_ == ' ').reverse + "|")
    println("abcdef".grouped(2).toList + " " + "abcd".sliding(3).toList + " " + "hello".indexWhere(_ == 'l') + " " + "hello".span(_ != 'l'))
    println(String.valueOf(3.5) + " " + String.valueOf(Array('x', 'y')) + " " + "a".concat("b") + " " + String.join("-", "p", "q") + " " + "x".repeat(3))
    println("Mississippi".toLowerCase.split("ss").toList + " " + "a:b:c".split(":", 2).toList + " " + "hello".updated(0, 'j') + " " + "hello".patch(1, "EE", 2))
    println("line1\nline2\n".linesIterator.toList + " " + "  ".isBlank + " " + "abc".codePointAt(1) + " " + "A".charAt(0).toInt + " " + 'a'.toUpper)
    val sb = new java.lang.StringBuilder; sb.append("j").append(1).append('c')
    println(sb.toString + " " + sb.reverse())
    println("hello" == "hel" + "lo") ; println(("hel" + "lo").intern eq "hello")
    val interned = "hello"; val built = new String("hello")
    println((interned == built) + " " + (interned eq built) + " " + interned.equals(built))
  }
}
