object Color extends Enumeration {
  val Red, Blue = Value
}
object Main {
  def main(args: Array[String]): Unit = {
    println(Color.Red.toString)
    println(Color.Red.id)
    println(Color.Blue.toString)
  }
}
