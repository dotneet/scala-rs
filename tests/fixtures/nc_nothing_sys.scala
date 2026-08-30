object Main {
  // The exact shape reported against real scalac: a `match` arm whose value
  // is a call statically typed `Nothing` (`sys.error`, not an explicit
  // `throw`), joining with a `Tuple2`-producing arm.
  def f(x: Int): (Int, Int) = x match { case 1 => (1, 1); case _ => sys.error("bad") }

  def g: Int = sys.error("x")

  def h(x: Int): Int = if (x > 0) x else sys.error("neg")

  def opt(x: Option[Int]): Int = x.getOrElse(sys.error("none"))

  def report(name: String)(body: => Any): Unit = {
    try {
      println(name + " = " + body)
    } catch {
      case e: Throwable => println(name + " threw " + e.getClass.getSimpleName)
    }
  }

  def main(args: Array[String]): Unit = {
    report("f(1)")(f(1)._1)
    report("f(2)")(f(2)._1)
    report("g")(g)
    report("h(1)")(h(1))
    report("h(-1)")(h(-1))
    report("opt(Some(1))")(opt(Some(1)))
    report("opt(None)")(opt(None))
  }
}
