// scala-rs rejects: `extends super.Val` inside an Enumeration (the
// no-argument Val constructor among its overloads).
object Main {
  object Planet extends Enumeration {
    protected case class PlanetVal(mass: Double) extends super.Val
    import scala.language.implicitConversions
    implicit def toPlanetVal(v: Value): PlanetVal = v.asInstanceOf[PlanetVal]
    val Mercury = PlanetVal(3.3e23); val Earth = PlanetVal(5.97e24)
  }
  def main(args: Array[String]): Unit = println(Planet.values.toList + " " + Planet.Earth.mass)
}
