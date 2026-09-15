import asynchook.{Generic, ForeignFuture}
import java.util.concurrent.{CompletableFuture, TimeUnit}
object Main {
  def preserve[T](value: T): Option[T] = Generic.optionally { Generic.value(Some(value)) }
  def optionReturn(): Any = {
    Generic.optionally { Generic.value(Some(1)); return "generic-return" }
    "unreachable"
  }
  def futureReturn(): Any = {
    val f = ForeignFuture.async[Any] { return "captured-return" }
    try f.get(5, TimeUnit.SECONDS) catch {
      case t: java.util.concurrent.ExecutionException =>
        println(t.getCause.asInstanceOf[scala.runtime.NonLocalReturnControl[_]].value)
    }
    "caller-continued"
  }
  def main(args: Array[String]): Unit = {
    println(Generic.optionally { val x = Generic.value(Some(40)); x + Generic.value(Some(2)) })
    println(Generic.optionally { val x = Generic.value(None: Option[Int]); x + 1 })
    println(Generic.optionally { 42 })
    println(optionReturn())
    println(preserve(42))
    println(preserve("typed"))
    println(Generic.optionally {
      var n = 0
      while (n < 10000) n += Generic.value(Some(1))
      n
    })
    println(Generic.optionally {
      val x = Generic.value(Some(2))
      if (x > 1) Generic.value(Some(42)) else Generic.value(None: Option[Int])
    })
    println(Generic.optionally {
      val n = try { throw new IllegalArgumentException("caught"); 0 } catch { case _: IllegalArgumentException => 40 }
      n + Generic.value(Some(2))
    })
    val input = new CompletableFuture[Int]()
    val output = ForeignFuture.async {
      val x = ForeignFuture.await(input)
      x + ForeignFuture.await(CompletableFuture.completedFuture(2))
    }
    println(output.isDone)
    input.complete(40)
    println(output.get(5, TimeUnit.SECONDS))
    val polled = ForeignFuture.polling { ForeignFuture.await(CompletableFuture.completedFuture(42)) }
    println(polled.get(5, TimeUnit.SECONDS))
    val pool = java.util.concurrent.Executors.newFixedThreadPool(2)
    try {
      var i = 0
      var total = 0
      while (i < 1000) {
        val p = new CompletableFuture[Int]()
        pool.execute(new Runnable { def run(): Unit = { p.complete(41); () } })
        val raced = ForeignFuture.async { ForeignFuture.await(p) + 1 }
        total += raced.get(5, TimeUnit.SECONDS)
        i += 1
      }
      println(total)
    } finally { pool.shutdown() }
    val failure = new CompletableFuture[Int]()
    val failed = ForeignFuture.async { ForeignFuture.await(failure) + 1 }
    failure.completeExceptionally(new IllegalArgumentException("failed-await"))
    try failed.get(5, TimeUnit.SECONDS) catch {
      case t: java.util.concurrent.ExecutionException => println(t.getCause.getMessage)
    }
    try ForeignFuture.propagating { throw new IllegalStateException("propagated"); 42 }
    catch { case t: IllegalStateException => println(t.getMessage) }
    val hookFailure = ForeignFuture.brokenComplete { 42 }
    try hookFailure.get(5, TimeUnit.SECONDS) catch {
      case t: java.util.concurrent.ExecutionException => println(t.getCause.getMessage)
    }
    println(futureReturn())
    val interrupted = ForeignFuture.async { throw new InterruptedException("interrupted"); 42 }
    try interrupted.get(5, TimeUnit.SECONDS) catch {
      case t: java.util.concurrent.ExecutionException => println(t.getCause.getMessage)
    }
    println(Thread.interrupted())
    try ForeignFuture.propagating { throw new InterruptedException("escaped-interrupt"); 42 }
    catch { case t: InterruptedException => println(t.getMessage) }
    println(Thread.interrupted())
    val assertion = ForeignFuture.async { throw new AssertionError("assertion"); 42 }
    try assertion.get(5, TimeUnit.SECONDS) catch {
      case t: java.util.concurrent.ExecutionException => println(t.getCause.getClass.getName)
    }
    val bodyFailed = ForeignFuture.async { throw new IllegalStateException("failed-body"); 42 }
    try bodyFailed.get(5, TimeUnit.SECONDS) catch {
      case t: java.util.concurrent.ExecutionException => println(t.getCause.getMessage)
    }
  }
}
