// Two abstract type members that bound each other. scalac 2.13.16:
// `cyclic aliasing or subtyping involving type X`. Neither member says
// anything, and every walk that replaces an abstract type by its upper bound
// runs between the two for ever.
object Main {
  trait A {
    type X <: Y
    type Y <: X
  }
  def main(args: Array[String]): Unit = ()
}
