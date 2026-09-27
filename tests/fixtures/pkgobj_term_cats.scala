// cats' `State` and `Reader` are both a type alias and an object in
// `package object data`. Written out in full, `cats.data.State.modify` has to
// reach the object; it reached the alias and typed the lambda's parameter as
// `IndexedStateT`'s unsolved `SB` (`value updated is not a member of S`).
package tf

trait Console[F[_]] { def put(s: String): F[Unit] }
trait Store[F[_]] { def get(k: String): F[Option[Int]] }

object Interp {
  type St[A] = cats.data.State[Map[String, Int], A]
  implicit val console: Console[St] =
    s => cats.data.State.modify(m => m.updated("log", m.getOrElse("log", 0) + s.length))
  implicit val store: Store[St] = k => cats.data.State.inspect(_.get(k))
  val seed: St[Int] = cats.data.State.pure(7)
  val reader: cats.data.Reader[Int, String] = cats.data.Reader((i: Int) => "r" + i)
}

object Main {
  import Interp._
  def main(args: Array[String]): Unit = {
    val prog = for {
      _ <- console.put("hello")
      a <- store.get("log")
      b <- seed
    } yield (a, b)
    println(prog.run(Map("k" -> 1)).value)
    println(reader.run(3))
  }
}
