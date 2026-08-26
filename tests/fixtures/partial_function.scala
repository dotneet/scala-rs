object Main {
  def main(args: Array[String]): Unit = {
    val pf: PartialFunction[Int, Int] = { case 1 => 2; case 2 => 3 }
    println(pf.isDefinedAt(1))
    println(pf.isDefinedAt(3))
    println(pf.apply(1))
    println(pf.apply(2))
    println(pf.applyOrElse(3, (x: Int) => 0))
  }
}
