import scala.reflect.ClassTag

object Main {
  def narrow[T <: Throwable](value: Throwable)(implicit tag: ClassTag[T]): String =
    value match {
      case _: T => "matched"
      case _ => "other"
    }

  // With no evidence, Scala retains the ordinary unchecked-erasure behavior:
  // this tests the upper bound (`Throwable`) and therefore matches.
  def unchecked[T <: Throwable](value: Throwable): String =
    value match {
      case _: T => "erased"
      case _ => "other"
    }

  def main(args: Array[String]): Unit = {
    println(narrow[IllegalArgumentException](new IllegalArgumentException))
    println(narrow[IllegalArgumentException](new IllegalStateException))
    println(narrow[IllegalArgumentException](null))
    println(unchecked[IllegalArgumentException](new IllegalStateException))
  }
}
