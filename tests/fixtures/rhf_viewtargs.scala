// An implicit conversion whose parameter is a *cake's inner class* must still
// solve its type parameters from the receiver.
//
// `profile.Comp[T, R]` is a class type under the as-seen-from view that records
// its prefix, and the unifier had no arm for that wrapper: both `T` and `R` were
// left open. blocking-slick's `implicit class ReturningInsertActionComposer2[T,
// R](a: ReturningInsertActionComposer[T, R])` is exactly this shape, so
// gitbucket's
//
//   val accountId = Accounts returning Accounts.map(_.accountId) insert account
//
// had result type `Nothing` and `createAccount` became an unconditional throw
// (`VerifyError: uninitialized … is not assignable to 'java/lang/Throwable'`).
trait Profile {
  class Comp[T, R](val t: T)

  trait API {
    implicit class Wrap[T, R](a: Comp[T, R]) {
      def insert(x: T): R = x.asInstanceOf[R]
      def twice(x: T): List[R] = List(insert(x), insert(x))
    }
  }
}

object P extends Profile {
  object MyApi extends API
  def comp[T, R](t: T): Comp[T, R] = new Comp[T, R](t)
}

object Main {
  import P.MyApi._

  def main(args: Array[String]): Unit = {
    val c: P.Comp[String, String] = P.comp[String, String]("seven")
    println(c.insert("seven").length)
    println(c.twice("x"))
    // A receiver written without the prefix, for the same conversion.
    val d = new P.Comp[Int, Int](3)
    println(d.insert(4) + 1)
  }
}
