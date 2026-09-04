// What scalac rejects in the numeric tower. Each has to draw a diagnostic.
object Main {
  def takeB(x: Byte): Int = x.toInt

  def main(args: Array[String]): Unit = {
    // Narrowing conversions do not happen implicitly (`toByte` has to be written).
    val i = 300
    val b: Byte = i

    // Even a constant cannot be narrowed when out of range (SLS 6.26.1).
    val b2: Byte = 300

    // An out-of-range constant cannot be passed to a Byte parameter (`takeB(3)` passes under SLS 6.26.1).
    println(takeB(300))

    // Boolean has no toX.
    println(true.toInt)

    // Neither does Unit.
    println(().toByte)

    // There is no weak conformance the other way (Double does not fall to Int).
    val n: Int = 1.5

    // Narrowing to Char does not happen implicitly either.
    val c: Char = i
  }
}
