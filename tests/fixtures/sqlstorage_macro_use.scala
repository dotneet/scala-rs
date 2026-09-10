object Main {
 def main(args: Array[String]): Unit = {
  val xs = List(1, 2, 3)
  println(MacroArgs.types(xs: _*))
  println(MacroArgs.types(1, 2))
  println(MacroArgs.types())
 }
}
