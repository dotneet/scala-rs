// `super.m`'s member types are seen from `this.type`, not from the parent
// named on its own. slick's `SQLiteProfile.SQLiteInsertAll.insertAll` calls
// `super.insertAll(values = …, rowsPerStatement = …)` on a parameter whose
// declared type is the *abstract* member `type RowsPerStatement >: One.type
// <: RowsPerStatement`, which the profile's `MultipleRowsPerStatementSupport`
// mixin refines to the concrete `slick.jdbc.RowsPerStatement`.

sealed trait Rps
object Rps {
  case object All extends Rps
  case object One extends Rps
}

/// Public face of the composers, free of abstract type members so the driver
/// below can name it.
trait Runner[U] {
  def run(value: U, batch: Boolean): String
}

trait Comp {
  type RowsPerStatement >: Rps.One.type <: Rps
  def defaultRows: RowsPerStatement

  // A second, already-concrete member: `super` must not lose the parent's own
  // alias where the current class refines nothing.
  type Label = String

  trait Composer[U] {
    def insertAll(value: U, batch: Boolean, rows: RowsPerStatement): String
    def label(l: Label): String
  }

  abstract class ComposerImpl[U] extends Composer[U] {
    def insertAll(value: U, batch: Boolean, rows: RowsPerStatement): String =
      "impl[" + value + "|" + rows + "]"
    def label(l: Label): String = "label(" + l + ")"
  }
}

trait MultiSupport extends Comp {
  override type RowsPerStatement = Rps
  override def defaultRows: Rps.All.type = Rps.All
}

trait SingleSupport extends Comp {
  override type RowsPerStatement = Rps.One.type
  override def defaultRows: Rps.One.type = Rps.One
}

trait MultiProfile extends Comp with MultiSupport {
  private trait InsertAll[U] extends ComposerImpl[U] with Runner[U] {
    // Named arguments, exactly as slick writes it. The `if` widens
    // `One.type` to `Rps`, which conforms to the refined member but not to
    // the abstract one.
    override def insertAll(value: U, batch: Boolean, rows: RowsPerStatement) =
      super.insertAll(
        value = value,
        batch = batch,
        rows =
          if (batch) Rps.One
          else rows
      )
    override def label(l: Label): String = "multi:" + super.label(l)
    def run(value: U, batch: Boolean): String =
      insertAll(value, batch, Rps.All) + " " + label("L")
  }
  def make[U]: Runner[U] = new InsertAll[U] {}
}

trait SingleProfile extends Comp with SingleSupport {
  private trait InsertOne[U] extends ComposerImpl[U] with Runner[U] {
    // Here the refinement is *narrower* than the declared upper bound, so the
    // trait's own erasure of `RowsPerStatement` is not the parent's.
    override def insertAll(value: U, batch: Boolean, rows: RowsPerStatement) =
      super.insertAll(value, batch, if (batch) Rps.One else rows)
    def run(value: U, batch: Boolean): String = insertAll(value, batch, Rps.One)
  }
  def make[U]: Runner[U] = new InsertOne[U] {}
}

object MultiP extends MultiProfile
object SingleP extends SingleProfile

// slick's `ast/Library.scala` writes `val / = new SqlOperator("/")`. A field
// name is an *unqualified name* (JVMS 4.2.2), so `/` has to be encoded the
// same way method names are; emitting it raw made the class unloadable.
class Ops {
  val / = "div"
  val + = "plus"
  var % = "mod"
  def show: String = "" + this./ + "," + this.+ + "," + this.%
}
object Ops {
  val * = "times"
}

object Main {
  def main(args: Array[String]): Unit = {
    println(MultiP.make[String].run("a", false))
    println(MultiP.make[String].run("b", true))
    println(SingleP.make[Int].run(7, true))
    println(SingleP.make[Int].run(8, false))
    val o = new Ops
    o.% = "MOD"
    println(o.show + "," + Ops.*)
  }
}
