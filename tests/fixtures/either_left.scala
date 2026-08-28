object Main {
  def main(args: Array[String]): Unit = {
    val l: Either[String, Int] = Left("boom")
    val r: Either[String, Int] = Right(7)
    println(l.left.get)
    println(l.left.getOrElse("none"))
    println(r.left.getOrElse("none"))
    println(l.left.toOption)
    println(r.left.toOption)
    println(l.left.toSeq)
    println(l.left.map((s: String) => s.length))
    println(r.left.map((s: String) => s.length))
    println(l.left.flatMap((s: String) => Left(s + "!")))
    println(l.left.exists((s: String) => s.length > 2))
    println(l.left.forall((s: String) => s.length > 9))
    l.left.foreach((s: String) => println(s))
    println(l.left.e)
    println(l.left.filterToOption((s: String) => s.length > 2))
  }
}
