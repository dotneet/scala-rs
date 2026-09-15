import scala.async.Async.{async, await}
import scala.concurrent.{Await, ExecutionContext, Future}
import scala.concurrent.ExecutionContext.Implicits.global
import scala.concurrent.duration.Duration
class AsyncFieldHolder(val base: Int) {
  val future = async {
    val x = await(Future.successful(base))
    var y = x
    y = y + await(Future.successful(2))
    y
  }
}
object Main {
  val fieldFuture = async { val x = await(Future.successful(20)); x + 22 }
  class Box(val value: Int)
  class Counter { var value: Int = 0 }
  def result[A](f: Future[A]): A = Await.result(f, Duration(10, "seconds"))
  def generic[A](f: Future[A]): Future[A] = async { await(f) }
  def main(args: Array[String]): Unit = {
    println(result(fieldFuture))
    println(result(new AsyncFieldHolder(40).future))
    println(result(generic(Future.successful("generic"))))
    println(result(async {
      var n = await(Future.successful(2))
      n = n + await(Future.successful(3))
      val a = Array(0)
      a(0) = await(Future.successful(n))
      val c = new Counter
      c.value = await(Future.successful(a(0) + 1))
      val box = new Box(await(Future.successful(c.value + 1)))
      box.value
    }))
    println(result(async {
      val x = await(Future.successful(40))
      def add(y: Int): Int = x + y
      class Local { def get: Int = add(2) }
      await(Future.successful(1))
      new Local().get + x - x
    }))
    println(result(async {
      val x: Any = if (await(Future.successful(false))) 1 else "else"
      x.toString
    }))
    println(result(async {
      val x: Any = await(Future.successful(2)) match {
        case 1 => await(Future.successful(1))
        case _ => await(Future.successful("match"))
      }
      x.toString
    }))
    println(result(async { await(Future.successful("cast": Any)).asInstanceOf[String] }))
    println(result(async { await(Future.successful(Future.successful(42))) match { case f => await(f) } }))
    println(result(async { await(await(Future.successful(Future.successful(42)))) }))
    // Explicit context expressions are evaluated once, before the body starts.
    var contexts = 0
    def context: ExecutionContext = { contexts += 1; global }
    println(result(async { await(Future.successful(20)) + await(Future.successful(22)) }(context)))
    println(contexts)
    object Ordinary {
      def async(x: Int): Int = x + 1
      def await(x: Int): Int = x + 1
    }
    println(Ordinary.async(Ordinary.await(40)))
  }
}
