object Main {
  def main(args: Array[String]): Unit = {
    val s: Option[Int] = Some(3)
    val n: Option[Int] = None
    println(s.getOrElse(0))
    println(n.getOrElse(0))
    println(s.isDefined)
    println(n.isDefined)
    println(s.nonEmpty)
    println(n.nonEmpty)
    println(s.contains(3))
    println(s.contains(4))
    println(n.contains(3))
    println(s.exists((x: Int) => x > 1))
    println(n.exists((x: Int) => x > 1))
    println(s.forall((x: Int) => x > 1))
    println(n.forall((x: Int) => x > 1))
    println(s.filter((x: Int) => x > 1).isEmpty)
    println(s.filter((x: Int) => x > 9).isEmpty)
    println(n.filter((x: Int) => x > 1).isEmpty)
    println(s.filterNot((x: Int) => x > 1).isEmpty)
    println(s.filterNot((x: Int) => x > 9).isEmpty)
    println(s.orElse(Some(9)).get)
    println(n.orElse(Some(9)).get)
    println(s.fold(0)((x: Int) => x + 1))
    println(n.fold(0)((x: Int) => x + 1))
  }
}
