// Matches that scalac compiles to switches: strings with colliding hash
// codes ("Aa" and "BB"), sparse and negative Int cases, Char and Byte
// cases, and a default that must not swallow a later case.
object Main {
  def str(s: String): Int = s match {
    case "Aa" => 1
    case "BB" => 2
    case "AaAa" => 3
    case "BBBB" => 4
    case "AaBB" => 5
    case "" => 6
    case null => 7
    case _ => 0
  }
  def sparse(i: Int): String = i match {
    case Int.MinValue => "min"
    case -1 => "minus one"
    case 0 => "zero"
    case 1000000 => "million"
    case Int.MaxValue => "max"
    case 3 | 5 | 7 => "small odd"
    case x if x > 100 => "big"
    case _ => "other"
  }
  def chr(c: Char): Int = c match { case 'a' => 1; case '\u00ff' => 2; case '\uffff' => 3; case _ => 0 }
  def byt(b: Byte): String = b match { case -128 => "min"; case 127 => "max"; case 0 => "zero"; case _ => "other" }
  def lng(l: Long): String = l match { case 1L => "one"; case 5000000000L => "5e9"; case -1L => "neg"; case _ => "?" }
  def main(args: Array[String]): Unit = {
    println(List("Aa", "BB", "AaAa", "BBBB", "AaBB", "BBAa", "", null, "x").map(str))
    println("Aa".hashCode == "BB".hashCode)
    println(List(Int.MinValue, -1, 0, 1000000, Int.MaxValue, 3, 4, 7, 101, 50).map(sparse))
    println(List('a', '\u00ff', '\uffff', 'b').map(chr))
    println(List[Byte](-128, 127, 0, 5).map(byt))
    println(List(1L, 5000000000L, -1L, 2L).map(lng))
    val sh: Short = 300
    println(sh match { case 300 => "three hundred"; case _ => "no" })
  }
}
