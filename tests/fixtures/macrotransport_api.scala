import scala.language.experimental.macros
object MacroTransport {
  def empty(): Int = macro MacroTransportImpl.empty
  def finalEmpty(a: Int)(): Int = macro MacroTransportImpl.finalEmpty
  def typeShapes: Int = macro MacroTransportImpl.typeShapes
  def block(a: Int): Int = macro MacroTransportImpl.block
  def silent: Int = macro MacroTransportImpl.silent
  def attached: Int = macro MacroTransportImpl.attached
  def repeated(args: Int*): Int = macro MacroTransportImpl.repeated
  def position: String = macro MacroTransportImpl.position
  def local: Int = macro MacroTransportImpl.local
  private def secret: Int = macro MacroTransportImpl.constant
}
package object macrotransportpkg {
  def answer: Int = macro MacroTransportImpl.constant
  def block(a: Int): Int = macro MacroTransportImpl.block
  private def secret: Int = macro MacroTransportImpl.constant
}
