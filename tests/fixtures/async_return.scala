import scala.async.Async.{async, await}
import scala.concurrent.{ExecutionContext, Future, Promise, Await}
import scala.concurrent.duration.Duration

object Main {
  val direct = new ExecutionContext {
    def execute(r: Runnable): Unit = r.run()
    def reportFailure(t: Throwable): Unit = throw t
  }
  def immediate(): Any = { async { return "immediate" }(direct); "unreachable" }
  def completed(): Any = { async { await(Future.successful(1)); return "completed" }(direct); "unreachable" }
  def returnedAwait(): Any = { async { return await(Future.successful("returned-await")) }(direct); "unreachable" }
  def delayed(p: Promise[Int]): Any = async { await(p.future); return "late-return" }(direct)
  def finalized(): Any = {
    async {
      await(Future.successful(1))
      try { return "finalized" } finally { println("finally") }
    }(direct)
    "unreachable"
  }
  def localReturn(): Future[Int] = async {
    def local(n: Int): Int = { if (n > 0) return n; 0 }
    local(await(Future.successful(42)))
  }(direct)
  def loopReturn(): Any = {
    async {
      var n = 0
      while (n < 3) { n += await(Future.successful(1)); if (n == 2) return "loop-return" }
    }(direct)
    "unreachable"
  }
  def typedReturn[T](value: T): T = {
    async { return value }(direct)
    throw new AssertionError("unreachable")
  }
  val signal = Promise[Int]()
  def reentrant(depth: Int): Any = {
    if (depth == 0) {
      async { await(signal.future); return "activation" }(direct)
      reentrant(1)
      "wrong-outer"
    } else { signal.success(1); "wrong-inner" }
  }
  def main(args: Array[String]): Unit = {
    println(immediate())
    println(completed())
    println(returnedAwait())
    println(finalized())
    println(Await.result(localReturn(), Duration(5, "seconds")))
    println(loopReturn())
    println(reentrant(0))
    println(typedReturn(42))
    println(typedReturn("generic"))
    val p = Promise[Int]()
    val f = delayed(p).asInstanceOf[Future[Any]]
    try p.success(1) catch { case t: scala.runtime.NonLocalReturnControl[_] => println("escaped:" + t.value) }
    println(f.isCompleted)
  }
}
