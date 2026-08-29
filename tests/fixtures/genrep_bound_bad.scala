// The namer resolves a class type parameter's bound before the unit's
// imports exist, so it may not report what it cannot find there. The class
// signature pass must still report a bound that names nothing at all.
package genrep.lifted {
  trait Rep[T]
}

package genrep.bad {
  import genrep.lifted._

  class Boxed[T <: Nope[_]](val rep: T)
}

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}
