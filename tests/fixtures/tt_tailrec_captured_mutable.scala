import scala.annotation.tailrec

class TailrecCapturedMutable {
  def run(limit: Int): Int = {
    var marker = "x"
    @tailrec
    def loop(n: Int): Int =
      if (n >= limit) n + marker.length
      else {
        marker = if (marker == "x") "yy" else "x"
        loop(n + 1)
      }
    loop(0)
  }
}

object Main {
  def main(args: Array[String]): Unit =
    println(new TailrecCapturedMutable().run(100000))
}
