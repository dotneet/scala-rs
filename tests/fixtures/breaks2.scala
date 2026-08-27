import scala.util.control.Breaks._
object Main {
  def main(args: Array[String]): Unit = {
    tryBreakable {
      var i = 0
      while (i < 5) {
        if (i == 3) break()
        println(i)
        i += 1
      }
    } catchBreak {
      println("caught")
    }
    println("after-break")
    tryBreakable {
      var i = 0
      while (i < 3) {
        println(i)
        i += 1
      }
    } catchBreak {
      println("should-not")
    }
    println("after-full")
    println(tryBreakable { 1 } catchBreak { 2 })
    println(tryBreakable { break(); 1 } catchBreak { 2 })
    val b = new scala.util.control.Breaks
    b.tryBreakable {
      b.break()
      println("no")
    } catchBreak {
      println("new-caught")
    }
    try {
      tryBreakable {
        throw new RuntimeException("boom")
      } catchBreak {
        println("swallowed")
      }
    } catch {
      case t: Throwable => println(t)
    }
  }
}
