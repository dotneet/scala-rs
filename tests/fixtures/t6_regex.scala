// `scala.util.matching.Regex` against the real library ABI.
//
// The prelude used to declare `findAllIn` / `findFirstMatchIn` /
// `replaceAllIn` / `replaceFirstIn` / `split` itself, as a fallback for a
// pickle that had not been read -- and since a jar member is only visible once
// something has asked for it, the fallback was what every call got. The
// results were `Any` (so `findFirstMatchIn(s).map(…)` was `value map is not a
// member of Any`) and the parameters were `String` where the library takes
// `CharSequence`, which links to nothing. Only `unapplySeq` is declared now;
// the rest come from the pickle with their real signatures, which is what the
// `replaceAllIn` line below checks -- it threw `NoSuchMethodError` before.

object Main {
  val re = "a(b+)c".r
  val Num = """(\d+)-(\d+)""".r

  def main(args: Array[String]): Unit = {
    println(re.findFirstMatchIn("xabbcy").map(m => m.group(1)).getOrElse("none"))
    println(re.findFirstMatchIn("zzz").map(m => m.group(1)).getOrElse("none"))
    println(re.findAllIn("abc abbc").size)
    println(re.findFirstIn("xabcy").getOrElse("none"))
    println(re.replaceAllIn("abc-abc", "Q"))
    println(re.replaceFirstIn("abc-abc", "Q"))
    println(re.split("1abc2").length)
    "12-34" match {
      case Num(a, b) => println(a + "/" + b)
      case _         => println("no match")
    }
  }
}
