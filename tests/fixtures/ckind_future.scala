// `scala.concurrent.Future` is not a prelude class, so every one of its
// members comes from the jar. `Future.apply` takes its body **by name**
// (`=> T`), which a JVM generic signature cannot say: there it is a plain
// `Function0[T]`, and `Future(21)` was "no matching overload for
// (Function0[T], ExecutionContext)Future[T] with arguments (21)".
//
// The shape is only reachable through the *companion*: `Future$#apply`. The
// class `Future` and the object `Future` used to share one symbol whose
// `jvm_name` was the companion's, so the pickle -- which does record by-name
// parameters -- could never be matched up with either.
//
// `parasitic` runs each callback on the thread that completes it, so the
// program is deterministic without `Await`.
//
// Library-ABI only: the private runtime has no `scala.concurrent` at all.

import scala.concurrent.{ExecutionContext, Future}

object Main {
  implicit val ec: ExecutionContext = ExecutionContext.parasitic

  def main(args: Array[String]): Unit = {
    // The companion's by-name `apply`.
    val f = Future(21)
    println(f.value.get.get)
    // A member of the class, on top of a second companion member.
    val g = Future.successful(2).map(_ * 10)
    println(g.value.get.get)
  }
}
