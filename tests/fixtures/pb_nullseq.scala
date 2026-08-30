// `null` against the pattern kinds that need the real library ABI: sequence
// patterns, an `Array` type pattern and the `Unit` constant.
object Main {
  def seqPat(x: Any): String = x match { case Seq(a, b) => s"seq $a $b"; case _ => "o" }
  def listPat(x: Any): String = x match { case a :: b :: Nil => s"list $a $b"; case _ => "o" }
  def arrPat(x: Any): String = x match {
    case a: Array[Int] => "ai" + a.length
    case _ => "o"
  }
  def unitPat(x: Any): String = x match { case () => "u"; case _ => "o" }
  def main(args: Array[String]): Unit = {
    println(seqPat(null)); println(seqPat(Seq(1, 2))); println(seqPat(Seq(1)))
    println(listPat(null)); println(listPat(List(1, 2))); println(listPat(List(1)))
    println(arrPat(null)); println(arrPat(Array(1, 2))); println(arrPat("s"))
    println(unitPat(null)); println(unitPat(())); println(unitPat(1))
  }
}
