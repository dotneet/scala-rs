import scala.reflect.runtime.universe._
object Bad { val x = definitions.TupleClass("wrong") }
