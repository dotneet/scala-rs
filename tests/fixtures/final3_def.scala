// agent/final3, part 2 of a pair; see `final3_use.scala`.
package final3def

trait Store {
  trait Handle { def label: String }
  def createEmpty: Handle = new Handle { def label = "empty" }
}
object Store extends Store

class Interp(db: Store#Handle, param: Any) {
  def run(n: Int): Any = "" + db.label + param + n
}
