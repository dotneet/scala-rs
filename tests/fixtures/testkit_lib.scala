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

/// A trait and a class *nested* in an object -- slick's
/// `object JdbcActionComponent { trait MultipleRowsPerStatementSupport }`,
/// which `trait H2Profile` names in its parent list.
///
/// This compiler writes one `ScalaSignature` per class file, so
/// `Support$Rows.class` carried a perfectly good pickle of `Rows` and
/// `Support.class` declared no members at all. nsc never looks in the nested
/// class file: it resolves `Support.Rows` as a *member* of `Support`'s own
/// signature, so it stopped at "Symbol 'type
/// slick.jdbc.JdbcActionComponent.MultipleRowsPerStatementSupport' is
/// missing from the classpath" the first time a parent list mentioned one.
object Support {
  trait Rows {
    def rows: Int
  }

  class Row(val n: Int)
}

/// The nested trait as a *parent*, one level of indirection away from the
/// object that uses it -- exactly how `H2Profile` reaches it.
trait WithRows extends Support.Rows {
  def rows: Int = 7
}

object Prof extends Base with Extra with WithRows {
  type API = Api
  val api: API = new Api {}
  /// The nested *class* in a member signature. nsc reports it separately from
  /// the parent-list case above ("Symbol 'type testkitlib.Support.Row' is
  /// missing from the classpath. This symbol is required by 'method
  /// testkitlib.Prof.firstRow'"), so both shapes are here.
  def firstRow: Support.Row = new Support.Row(1)
  val rowN: Int = firstRow.n
}
