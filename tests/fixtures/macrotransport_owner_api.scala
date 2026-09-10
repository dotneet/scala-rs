import scala.language.experimental.macros
object OwnerApi {
  def wrap(a: Int): Int = macro OwnerImpl.wrap
  def recursive(a: Int): Int = macro OwnerImpl.recursive
}
