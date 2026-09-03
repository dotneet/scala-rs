// Exceptions: the value of try/finally, Try, NonFatal, and a resource-closing
// helper of the shape `Using` / slick's session cleanup.
import scala.util.{Try, Success, Failure}
import scala.util.control.NonFatal

object Main {
  class Res(val name: String) {
    var closed = false
    def close(): Unit = { closed = true; println("close " + name) }
  }

  // NB: naming this `using` is rejected by scalac 2.13.16 itself
  // ("Main.Res does not take parameters"); `using` is a soft keyword there.
  def withRes[R <: Res, A](r: R)(f: R => A): A = try f(r) finally r.close()

  def value1(): Int = try { 1 } finally { println("fin1") }
  def value2(): Int = try { throw new RuntimeException("x") } catch { case _: RuntimeException => 2 } finally { println("fin2") }
  def value3(): Int = { var i = 0; try { i = 1; return i } finally { i = 2; println("fin3 " + i) } }
  def value4(): String = try { "a" } catch { case NonFatal(e) => "b" } finally { println("fin4") }

  def parse(s: String): Try[Int] = Try(s.toInt)

  def guard(n: Int): String =
    try {
      if (n < 0) throw new IllegalArgumentException("neg " + n)
      if (n == 0) throw new NoSuchElementException("zero")
      "ok " + n
    } catch {
      case e: IllegalArgumentException => "iae:" + e.getMessage
      case NonFatal(e) => "nf:" + e.getClass.getSimpleName
    }

  def main(args: Array[String]): Unit = {
    println(value1())
    println(value2())
    println(value3())
    println(value4())
    println(guard(3)); println(guard(-1)); println(guard(0))

    println(parse("12"))
    println(parse("x").isFailure)
    println(parse("x").getOrElse(-1))
    println(parse("5").map(_ * 2).filter(_ > 5))
    println(parse("5").flatMap(a => parse("6").map(a + _)))
    println(parse("q") match { case Success(v) => "s" + v; case Failure(e) => "f:" + e.getClass.getSimpleName })
    println(parse("q").recover { case _: NumberFormatException => 0 })
    println(Try { throw new Exception("e1") }.toOption)
    println(parse("3").toEither.isRight)

    val r = new Res("a")
    println(withRes(r)(x => x.name.length))
    println(r.closed)
    val r2 = new Res("b")
    val caught = try withRes(r2)(_ => throw new RuntimeException("inner")) catch { case e: RuntimeException => e.getMessage }
    println(caught + " " + r2.closed)

    val seq = List("1", "x", "3").map(parse)
    println(seq.collect { case Success(v) => v }.sum)
    println(seq.count(_.isSuccess))
    var trail = List.empty[String]
    try { trail ::= "t"; throw new Exception("z") } catch { case _: Throwable => trail ::= "c" } finally { trail ::= "f" }
    println(trail.reverse)
  }
}
