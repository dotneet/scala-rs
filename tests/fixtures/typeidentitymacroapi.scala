package typeidentitymacro
import scala.language.experimental.macros
trait Evidence[A] { def value: Int }
class EvidenceValue[A](val value: Int) extends Evidence[A]
trait LowPriority {
  implicit def fallback[A]: Evidence[A] = new Evidence[A] { def value: Int = 99 }
}
object Evidence extends LowPriority {
  implicit def chosen[A]: Evidence[A] = macro Impl.choose[A]
}
object API {
  def names[A, B]: String = macro Impl.names[A, B]
  def value: Int = macro Impl.value
  def sum(x: Int)(y: Int): Int = macro Impl.sum
}
class Owned[A] { def names[B]: String = macro Impl.names[A, B] }
