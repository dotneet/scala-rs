// agent/testkit, part 2: what a scala-rs classfile tells the *next*
// compilation.
//
// `CLASSINFOtpe` was written with exactly one parent -- `java.lang.Object` --
// for every class this compiler emitted. Nothing inherited was therefore
// visible to a later run: `import Prof._` brought in neither `greeting` nor
// `twice` nor `api`, and real scalac reading the same classfiles reported the
// same thing, which is what placed the bug in the writer.
//
// Compiled on its own; `testkit_use.scala` is compiled against the classfiles.
package testkitlib

trait Api {
  def col(n: String): Int = n.length
}

/// A bounded abstract type member: its bound is what tells a reader that
/// `api` has a `col` at all. The writer used to pickle every abstract type as
/// `Nothing .. Any`.
trait Base {
  type API <: Api
  val api: API
}

trait Extra {
  val greeting: String = "hi"
  def twice(i: Int): Int = i * 2
}

object Prof extends Base with Extra {
  type API = Api
  val api: API = new Api {}
}
