// Inference through a *bounded* existential on both sides. The four
// `invokeAll`/`invokeAny` forwarders in
// `scala/concurrent/impl/ExecutionContextImpl.scala` re-declare Java's
// `<T> invokeAll(Collection<? extends Callable<T>>)` and forward to the same
// method on another `ExecutorService`, so the argument is itself a
// `Collection[_ <: Callable[U]]` and `T := U` is readable only by lining the
// two wildcards' *bounds* up.
import java.util.Collection
import java.util.concurrent.{Callable, Executors, TimeUnit}

object Main {
  def want[T](cs: Collection[_ <: Callable[T]]): Int = cs.size
  def forward[U](cs: Collection[_ <: Callable[U]]): Int = want(cs)

  def main(args: Array[String]): Unit = {
    val es = Executors.newSingleThreadExecutor()
    try {
      val tasks = new java.util.ArrayList[Callable[String]]()
      tasks.add(new Callable[String] { def call(): String = "a" })
      tasks.add(new Callable[String] { def call(): String = "b" })
      println(forward(tasks))
      // The real shape: a Java generic method with a bounded-wildcard
      // parameter, called with a bounded-wildcard argument.
      def invokeAll[T](cs: Collection[_ <: Callable[T]]) = es.invokeAll(cs)
      def invokeAllTimed[T](cs: Collection[_ <: Callable[T]], l: Long, u: TimeUnit) =
        es.invokeAll(cs, l, u)
      println(invokeAll(tasks).size)
      println(invokeAllTimed(tasks, 10L, TimeUnit.SECONDS).get(0).get())
    } finally es.shutdown()
  }
}
