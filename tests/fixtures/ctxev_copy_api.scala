package importedcopy
case class Ordinary(value: Int)
case class Custom(value: Int) { def copy(text: String): String = text }
case class Private(value: Int) { private def copy(text: String): String = text }
trait ConcreteCopy { def copy(text: String): String = text }
case class Inherited(value: Int) extends ConcreteCopy
class Restricted {
  private def secret(text: String): String = text
  protected def inherited(text: String): String = text
}
