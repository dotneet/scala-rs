// Every `scala.language` feature name must be importable, in every shape.
import scala.language.{implicitConversions, existentials}
import scala.language.higherKinds
import scala.language.reflectiveCalls
import scala.language.experimental.macros

object Main {
  def main(args: Array[String]): Unit = println("ok")
}
