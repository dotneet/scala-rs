object Main {
  def die(): Nothing = throw new RuntimeException("boom")

  def matchArm(x: Int): (Int, Int) =
    x match { case 1 => (1, 1); case _ => die() }

  def ifArm(x: Int): Int =
    if (x > 0) x else die()

  def tryArm(x: Int): Int =
    try { x / (x - x) } catch { case _: ArithmeticException => die() }

  def blockTail(): Int = { println("before"); die() }

  def wholeBody: Int = die()

  def ascribed: Int = (??? : Int)

  def takeAny(a: Any): Unit = ()
  def argPosition(): Unit = takeAny(die())

  def explicitThrowArm(x: Int): (Int, Int) =
    x match { case 1 => (1, 1); case _ => throw new RuntimeException("explicit") }

  def report(name: String)(body: => Any): Unit = {
    try {
      println(name + " = " + body)
    } catch {
      case e: Throwable => println(name + " threw " + e.getClass.getSimpleName)
    }
  }

  def main(args: Array[String]): Unit = {
    // Report tuple arms through `._1`: the private runtime's `Tuple2` has no
    // `toString` override, so printing the tuple itself would diverge from
    // `--scala-library` output on a detail unrelated to this fixture.
    report("matchArm(1)")(matchArm(1)._1)
    report("matchArm(2)")(matchArm(2)._1)
    report("ifArm(1)")(ifArm(1))
    report("ifArm(-1)")(ifArm(-1))
    report("tryArm(1)")(tryArm(1))
    report("blockTail")(blockTail())
    report("wholeBody")(wholeBody)
    report("ascribed")(ascribed)
    report("argPosition")(argPosition())
    report("explicitThrowArm(1)")(explicitThrowArm(1)._1)
    report("explicitThrowArm(2)")(explicitThrowArm(2)._1)
  }
}
