object Main {
  def main(args: Array[String]): Unit = {
    val s: Option[Int] = Some(3)
    val n: Option[Int] = None
    println(s.toList)
    println(n.toList)
    println(s.toRight("empty"))
    println(n.toRight("empty"))
    println(s.toLeft("empty"))
    println(n.toLeft("empty"))
    println(s.zip(Some(4)))
    println(s.zip(n))
    println(s.collect { case 3 => "three" })
    println(s.collect { case 4 => "four" })
    val nested: Option[Option[Int]] = Some(Some(5))
    println(nested.flatten)
  }
}
