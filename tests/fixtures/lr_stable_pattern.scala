object Main {
  def main(args: Array[String]): Unit = {
    var evaluations = 0
    lazy val x = { evaluations += 1; 3 }
    def matched(value: Any): Boolean = value match {
      case `x` => true
      case _ => false
    }
    println(evaluations)
    println(matched(2))
    println(matched(3))
    println(matched("3"))
    val pf: PartialFunction[Any, String] = { case `x` => "hit" }
    println(pf.isDefinedAt(2))
    println(pf.isDefinedAt(3))
    println(pf(3))
    println(evaluations)
  }
}
