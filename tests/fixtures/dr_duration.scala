// The postfix units of scala.concurrent.duration (the first agent/durrange case).
// The implicit defs DurationInt / DurationLong / DurationDouble of
// `package object duration`, and every unit method of DurationConversions.
// Needs the real scala-library jar (--scala-library only).
import scala.concurrent.duration._

object Main {
  def main(args: Array[String]): Unit = {
    // Int: all 20 unit methods (singular, plural and abbreviated).
    println(List(1.nanoseconds, 1.nanos, 1.nanosecond, 1.nano).mkString(" "))
    println(List(2.microseconds, 2.micros, 2.microsecond, 2.micro).mkString(" "))
    println(List(3.milliseconds, 3.millis, 3.millisecond, 3.milli).mkString(" "))
    println(List(4.seconds, 4.second).mkString(" "))
    println(List(5.minutes, 5.minute).mkString(" "))
    println(List(6.hours, 6.hour).mkString(" "))
    println(List(7.days, 7.day).mkString(" "))
    // Long and Double take the same path.
    println(List(8L.seconds, 8L.millis, 9L.days, 10L.hour).mkString(" "))
    println(List(1.5d.seconds, 0.25d.millis, 2.0d.minutes).mkString(" "))
    // Interop and arithmetic with FiniteDuration / Duration.
    val f: FiniteDuration = 2.seconds
    val d: Duration = f
    println(f.toMillis.toString + " " + d.toString)
    println((1.second + 500.millis).toString)
    println((3.seconds - 1.second).toString)
    println(Duration(5, SECONDS).toString + " " + Duration.Inf.toString)
  }
}
