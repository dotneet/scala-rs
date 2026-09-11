// Char: arithmetic yields Int, comparisons, RichChar predicates and
// conversions, char ranges, and chars as map keys and match scrutinees.
object Main {
  def kind(c: Char): String = c match {
    case 'a' | 'e' | 'i' | 'o' | 'u' => "vowel"
    case d if d.isDigit => "digit"
    case ' ' => "space"
    case '\n' => "newline"
    case u if u.isUpper => "upper"
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    val c = 'm'
    println(c + 1); println((c + 1).toChar); println(c - 'a'); println(c * 2); println(c / 2); println(c.toInt); println(-c)
    println(('a' to 'e').toList + " " + ('a' until 'e' by 2).mkString + " " + ('a' to 'z' by 5).mkString + " " + ('a' to 'c').map(_.toUpper))
    println(c.isLetter + " " + c.isDigit + " " + '7'.asDigit + " " + 'Z'.toLower + " " + 'q'.toUpper + " " + ' '.isWhitespace + " " + 'A'.isUpper + " " + 'b'.isLower)
    println(Character.isLetterOrDigit('_') + " " + Character.getNumericValue('9') + " " + Character.toChars(66).mkString + " " + 'x'.getType + " " + Character.isJavaIdentifierStart('$'))
    println("hello world 42\n".map(kind).mkString(","))
    println(('a' < 'b') + " " + ('a' == 97) + " " + ('a'.compare('c')) + " " + ('a' max 'z') + " " + List('c', 'a', 'b').max)
    val counts = "banana".foldLeft(Map.empty[Char, Int])((m, ch) => m.updated(ch, m.getOrElse(ch, 0) + 1))
    println(counts.toList.sorted)
    val shifted = "abcxyz".map(ch => ((ch - 'a' + 3) % 26 + 'a').toChar)
    println(shifted)
    var ch = 'A'
    val sb = new StringBuilder
    while (ch <= 'E') { sb.append(ch); ch = (ch + 1).toChar }
    println(sb)
    val arr = Array('d', 'a', 'c')
    println(arr.sorted.mkString + " " + arr.max + " " + arr.map(_.toInt).sum)
    println('\u00e9' + " " + '\t'.toInt + " " + '\\' + " " + '\'' + " " + "\"q\"")
    val ci: Int = 'x'; val cl: Long = 'y'; val cd: Double = 'z'
    println(ci + " " + cl + " " + cd)
    println(('a' + 'b') + " " + ('a'.toString + 'b') + " " + s"${'a'}${'b'}" + " " + ("" + 'a' + 'b'))
    println(Char.MinValue.toInt + " " + Char.MaxValue.toInt + " " + (Char.MaxValue + 1) + " " + (Char.MaxValue + 1).toChar.toInt)
    val digits = "a1b2c3".filter(_.isDigit).map(_.asDigit).sum
    println(digits)
    println("Hello".map(ch => if (ch.isUpper) ch.toLower else ch.toUpper))
    println("abc".exists(_ == 'b') + " " + "abc".indexOf('c') + " " + "abc".contains('z'))
  }
}
