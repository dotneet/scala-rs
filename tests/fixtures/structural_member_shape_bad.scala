import scala.language.reflectiveCalls
class Box { def length: Int = 7 }
class UnitBox { def length(): Int = 8 }
class VBox { val length: Int = 1 }
class PBox { def length(x: Int): Int = x }
class OBox { def length(x: Int): Int = x; def length: Int = 2 }
object Shapes {
  val ok1: AnyRef { def length: Int } = new Box
  val ok2: AnyRef { def length(): Int } = new UnitBox
  val ok3: AnyRef { def length: Int } = new VBox
  val ok4: AnyRef { def length: Int } = new OBox
  val ok5: AnyRef { def length: Int } = List(1)
  val bad1: AnyRef { def length(): Int } = new Box
  val bad2: AnyRef { def length: Int } = new UnitBox
  val bad3: AnyRef { def length(): Int } = new VBox
  val bad4: AnyRef { def length(): Int } = new PBox
  val bad5: AnyRef { def length(): Int } = List(1)
}
