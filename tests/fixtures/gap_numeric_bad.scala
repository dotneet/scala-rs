// Numeric companion constants (`Int.MaxValue`, ...) are backed by the real
// scala-library ABI (scala/Int$.MODULE$.MaxValue()); the private runtime used
// with --no-scala-library has no such classfile, so this must be diagnosed
// rather than silently miscompiled.
object Main {
  def main(args: Array[String]): Unit = {
    println(Int.MaxValue)
  }
}
