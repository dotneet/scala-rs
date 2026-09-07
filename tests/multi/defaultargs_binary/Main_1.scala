// The consumer half: every default below is filled from a getter that lives
// in a separately compiled class file.
import dalib._

object Main extends Control {
  def main(args: Array[String]): Unit = {
    println(Box[String]().show)
    println(Box[Int](size = 1).show)
    println(Box[Long]("u", 2, false).show)
    println(halt(404))
    println(halt())
    println(halt(500, "body", "reason"))
    println(halt(true))
    println(Plain.join("a")()())
    println(Plain.join("a")("+")())
    println(Plain.join("a")("+")("c"))
  }
}
