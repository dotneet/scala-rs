import scala.async.Async.{async, await}
import scala.concurrent.{Await, ExecutionContext, Future, Promise}
import scala.concurrent.duration.Duration
import java.util.concurrent.Executors

object Main {
  def result[A](f: Future[A]): A = Await.result(f, Duration(10, "seconds"))
  def main(args: Array[String]): Unit = {
    val pool = Executors.newSingleThreadExecutor()
    implicit val ec: ExecutionContext = ExecutionContext.fromExecutor(pool)
    try {
      println(result(async { 42 }))
      println(result(async { val x = await(Future.successful(40)); x + 2 }))
      // Completion must be able to run on the same single-thread executor.
      val p = Promise[Int]()
      val waiting = async { await(p.future) + 2 }
      val completing = Future { p.success(40); () }
      println(result(waiting))
      result(completing)
      println(result(async {
        var events = ""
        def mark(s: String): Int = { events = events + s; 10 }
        val first = mark("a")
        val sum = first + await(Future { mark("b") }) + mark("c")
        events + ":" + sum
      }))
      println(result(async {
        val choose = await(Future.successful(true))
        val x = if (choose) await(Future.successful(20)) else await(Future.failed[Int](new Exception("unreachable")))
        val y = await(Future.successful(2)) match {
          case 1 => await(Future.successful(1))
          case n if n > 1 => await(Future.successful(n + 20))
          case _ => 0
        }
        x + y
      }))
      println(result(async {
        var n = 0
        var sum = 0
        while (await(Future.successful(n < 10000))) {
          sum = sum + await(Future.successful(1))
          n += 1
        }
        sum
      }))
      println(result(async {
        var n = 0
        do { n = n + await(Future.successful(1)) } while (n < 3)
        n
      }))
      println(result(async {
        var n = 0
        val x = false && await(Future { n += 1; true })
        val y = true || await(Future { n += 1; false })
        x.toString + ":" + y + ":" + n
      }))
      println(result(async { await(async { await(Future.successful(41)) + 1 }) }))
      println(result(async { await(Future.failed[Int](new IllegalArgumentException("await-failed"))); 0 }.failed).getMessage)
      println(result(async { await(Future.successful(1)); throw new IllegalArgumentException("body-failed") }.failed).getMessage)
      println(result(async { throw new IllegalArgumentException("start-failed") }.failed).getMessage)
      println(result(async {
        await(Future.successful(2)) match {
          case n if await(Future.successful(false)) => 0
          case n if await(Future.successful(n == 2)) => await(Future.successful(42))
          case _ => -1
        }
      }))
      println(result(async { try { 42 } finally { () } }))
      import scala.async.Async.{async => later, await => value}
      println(result(later[Int] { value(Future.successful(42)) }))
    } finally { pool.shutdown() }
  }
}
