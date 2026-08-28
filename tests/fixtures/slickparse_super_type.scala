// `super.T` in *type* position: a path to a type member of a parent, the way
// slick writes `override def createUpsertBuilder(node: Insert): super.InsertBuilder`.
// Covers a result type, a parameter type, a local `val`, a type alias, a
// parent in an `extends` clause, and the qualified `C.super` spelling.
trait Base {
  class Builder(val n: Int) {
    def show: String = "base-" + n.toString
  }
  type Alias = String
  def make(n: Int): Builder = new Builder(n)
  def wrap(b: Builder): String = "wrap(" + b.show + ")"
}

trait Mid extends Base {
  class MidBuilder(m: Int) extends super.Builder(m) {
    override def show: String = "mid-" + m.toString
  }
  override def make(n: Int): super.Builder = new MidBuilder(n)
  def alias(s: super.Alias): String = "alias-" + s
  def viaSuper(n: Int): String = {
    val b: super.Builder = new MidBuilder(n)
    b.show
  }
}

object Main extends Mid {
  def main(args: Array[String]): Unit = {
    println(make(1).show)
    println(wrap(make(2)))
    println(alias("x"))
    println(viaSuper(3))
    println(Main.super.make(4).show)
  }
}
