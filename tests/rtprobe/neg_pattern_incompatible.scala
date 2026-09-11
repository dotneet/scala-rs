// scalac: scrutinee is incompatible with pattern type; found String,
// required Int.
object Main {
  def main(args: Array[String]): Unit = println((1: Int) match { case s: String => s; case _ => "other" })
}
