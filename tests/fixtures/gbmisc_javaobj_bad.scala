// The folding is for a *Java* declaration's `Object` only: a different type
// argument still overrides nothing, and a Scala-declared `Map[String, Any]`
// is not implemented by `Map[String, AnyRef]` (invariant `Map`).
trait SMig {
  def migrate(moduleId: String, version: String, context: java.util.Map[String, Any]): Unit
}
object Main {
  def main(args: Array[String]): Unit = {
    val m1 = new GbmiscMig() {
      override def migrate(moduleId: String, version: String, context: java.util.Map[String, String]): Unit = ()
    }
    val m2 = new SMig {
      override def migrate(moduleId: String, version: String, context: java.util.Map[String, AnyRef]): Unit = ()
    }
  }
}
