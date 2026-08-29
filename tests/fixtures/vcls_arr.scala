final class Meters(val n: Int) extends AnyVal {
  def describe: String = n + "m"
}

case class Leg(m: Meters, label: String)

object Main {
  def id[A](a: A): A = a

  def main(args: Array[String]): Unit = {
    val arr = Array(new Meters(1), new Meters(2))
    println(arr.length)
    println(arr(0).n)
    println(arr.mkString(","))

    val fresh = new Array[Meters](2)
    fresh(0) = new Meters(7)
    println(fresh(0).describe)

    val xs: List[Meters] = List(new Meters(3), new Meters(4))
    println(xs.map(_.n).sum)
    println(xs.mkString(";"))
    println(xs.head.describe)

    val o: Option[Meters] = Some(new Meters(9))
    println(o.get.describe)
    println(o.map(_.n).getOrElse(0))

    println(id(new Meters(11)).describe)

    val leg = Leg(new Meters(3), "b")
    println(leg)
    println(leg.m.n)
    println(leg == Leg(new Meters(3), "b"))
    println(Set(new Meters(3), new Meters(3)).size)
  }
}
