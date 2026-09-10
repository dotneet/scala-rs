import scala.reflect.runtime.universe._
object Main {
  def missing[A]: Type = typeOf[A]
}
