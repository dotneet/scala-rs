import scala.util.control.Breaks._
object Main {
  def main(args: Array[String]): Unit = {
    breakable {
      var i = 0
      while (i < 5) {
        if (i == 3) break()
        println(i)
        i += 1
      }
    }
    println("done")
    breakable {
      var i = 0
      while (i < 3) {
        println(i)
        i += 1
      }
    }
    println("full")
    val b = new scala.util.control.Breaks
    b.breakable {
      var i = 0
      while (i < 5) {
        if (i == 2) b.break()
        println(i)
        i += 1
      }
    }
    println("new")
    try {
      break()
      println("after")
    } catch {
      case t: Throwable => println(t.getClass.getName)
    }
  }
}
