// Regex: pattern matching with groups, unanchored, findAllIn, findFirstMatchIn,
// replaceAllIn with a function, split, and matching in for comprehensions.
object Main {
  val Date = """(\d{4})-(\d{2})-(\d{2})""".r
  val KV = """(\w+)=(\w*)""".r
  val Num = """-?\d+""".r
  def main(args: Array[String]): Unit = {
    "2024-03-15" match { case Date(y, m, d) => println(s"$d/$m/$y"); case _ => println("no") }
    "on 2024-03-15 ok" match { case Date(y, _, _) => println("anchored " + y); case _ => println("anchored: no match") }
    println(Num.findAllIn("a1 b-22 c333").toList + " " + Num.findFirstIn("x") + " " + Num.findFirstIn("x9y"))
    println(KV.findAllMatchIn("a=1 b= c=3").map(m => m.group(1) + ":" + m.group(2)).toList)
    println(Num.replaceAllIn("a1b22", m => (m.matched.toInt * 2).toString) + " " + "a.b.c".replaceAll("\\.", "/"))
    println(Date.findFirstMatchIn("x 1999-12-31 y").map(m => m.start + "-" + m.end + " " + m.group(2)))
    println("a1b2c3".split("\\d").toList + " " + """\s+""".r.split("a  b \t c").toList)
    println((for (KV(k, v) <- List("x=1", "bad", "y=")) yield k + "->" + v))
    println(Num.matches("-42") + " " + Num.matches("4a") + " " + "abc".matches("[a-c]+"))
    val Words = """(\w+)\s(\w+)""".r
    val Words(first, second) = "hello world"
    println(second + " " + first)
    println("""(?i)HELLO""".r.findFirstIn("say hello") + " " + "x+y".replace("+", "-"))
    val all = """(\w)(\d)""".r.findAllIn("a1b2").matchData.map(m => m.group(2) + m.group(1)).mkString
    println(all)
    println("CamelCaseWords".split("(?=[A-Z])").toList)
    val opt = """(\d+)(?:px)?""".r
    println(List("10px", "20", "x").collect { case opt(n) => n.toInt })
  }
}
