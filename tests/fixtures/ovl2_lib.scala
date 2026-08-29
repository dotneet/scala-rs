// Overload resolution that only the real scala-library backs: an overloaded
// method named where a function type is expected, and the `ArrayBuffer`
// constructor alternatives.

import scala.collection.mutable.ArrayBuffer
import java.time.{Instant, LocalDate}
import java.time.format.DateTimeFormatter

object Main {
  // The overloaded `math.min` / `math.max` have to be narrowed to the one
  // alternative that eta-expands to the parameter's function type -- both in
  // argument position and against an expected type.
  def constOp[T](name: String)(f: (T, T) => T)(a: T, b: T): T = f(a, b)

  def main(args: Array[String]): Unit = {
    println(constOp[Long]("min")(math.min)(3L, 4L))
    println(constOp[Int]("max")(math.max)(3, 4))
    val g: (Double, Double) => Double = math.max
    println(g(1.5, 2.5))

    // `def this(initialSize: Int)` next to `def this()`.
    val b = new ArrayBuffer[Int](8)
    b += 1
    b += 2
    println(b.mkString(","))
    val c = new ArrayBuffer[String]()
    c += "x"
    println(c.mkString(","))

    // `String <: CharSequence`, so the JDK's `CharSequence` overloads apply.
    println(Instant.parse("2020-01-02T03:04:05Z"))
    println(LocalDate.parse("2020-01-02", DateTimeFormatter.ISO_LOCAL_DATE))
    println(DateTimeFormatter.ISO_LOCAL_DATE.parse("2021-02-03").isSupported(java.time.temporal.ChronoField.YEAR))
  }
}
