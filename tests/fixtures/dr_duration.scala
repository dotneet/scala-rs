// scala.concurrent.duration の後置単位（agent/durrange の 1 件目）。
// `package object duration` の implicit def DurationInt / DurationLong /
// DurationDouble と、DurationConversions の単位メソッド全部。
// 実 scala-library の jar が要る（--scala-library 専用）。
import scala.concurrent.duration._

object Main {
  def main(args: Array[String]): Unit = {
    // Int: 20 個の単位メソッド全部（単数形・複数形・短縮形）。
    println(List(1.nanoseconds, 1.nanos, 1.nanosecond, 1.nano).mkString(" "))
    println(List(2.microseconds, 2.micros, 2.microsecond, 2.micro).mkString(" "))
    println(List(3.milliseconds, 3.millis, 3.millisecond, 3.milli).mkString(" "))
    println(List(4.seconds, 4.second).mkString(" "))
    println(List(5.minutes, 5.minute).mkString(" "))
    println(List(6.hours, 6.hour).mkString(" "))
    println(List(7.days, 7.day).mkString(" "))
    // Long と Double も同じ経路。
    println(List(8L.seconds, 8L.millis, 9L.days, 10L.hour).mkString(" "))
    println(List(1.5d.seconds, 0.25d.millis, 2.0d.minutes).mkString(" "))
    // FiniteDuration / Duration との相互運用と算術。
    val f: FiniteDuration = 2.seconds
    val d: Duration = f
    println(f.toMillis.toString + " " + d.toString)
    println((1.second + 500.millis).toString)
    println((3.seconds - 1.second).toString)
    println(Duration(5, SECONDS).toString + " " + Duration.Inf.toString)
  }
}
