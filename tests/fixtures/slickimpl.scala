// Three shapes gitbucket is built out of, with no jar involved. Every one of
// them is about *when* a signature is available, not about what it says.
//
//  1. `import api._` where `api` is a `val` of the same template: the members
//     below it are typed against that import.
//  2. A deferred `val` implemented by a trait that reaches it through a self
//     type, mixed in beside the trait that declares it.
//  3. An import that walks through that object from another template, forcing
//     the inferred `lazy val` during the header pass.

trait Api {
  type Row[T] = List[T]
  type Plain = Int
  def wrap(n: Int): Row[Int] = List(n)
}

object Apis { val std: Api = new Api {} }

trait UsesApi {
  val api: Api
  import api._

  // Typed in the signature pass, when `api` still has no signature of its own.
  def one(r: Row[String]): Int = r.size
  def two(p: Plain): Int = p
}

// `Provider` is not a subclass of `UsesApi`; it only sees it through the self
// type. In `Live`, which has both, this `api` implements that `api`.
trait Provider { self: UsesApi =>
  lazy val api = Apis.std
}

object Live extends Provider with UsesApi

// The prefix walks `Live.api`, whose type is inferred from another object.
object Reader {
  import Live.api._
  def three(r: Row[Int]): Int = r.sum
  def four: Row[Int] = wrap(4)
}

object SlickImplMain {
  def main(args: Array[String]): Unit = {
    println(Live.one(List("a", "b")))
    println(Live.two(3))
    println(Reader.three(List(1, 2, 3)))
    println(Reader.four)
  }
}
