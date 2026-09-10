import java.sql._
import java.time._
object Main {
  def main(args: scala.Array[String]): Unit = {
    println(DriverManager.getLoginTimeout)
    println(DayOfWeek.MONDAY.getValue)
    println(Integer.parseInt("23"))
  }
}
