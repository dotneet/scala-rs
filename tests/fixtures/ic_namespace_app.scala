import scala.extensionsample._
import scala.concurrent.duration._
object Main {
  def main(args: Array[String]): Unit = {
    println("record".bracketed)
    println(3.seconds.toMillis)
  }
}
