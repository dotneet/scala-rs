import scala.language.experimental.macros
trait Parent[+A]
class LocalBox[+A <: CharSequence](val value: A) extends Parent[A]
object SourceTypes {
  def inspect[A]: String = macro SourceTypesImpl.inspect[A]
  def shape[A]: String = macro SourceTypesImpl.shape[A]
  def conforms[A, B]: Boolean = macro SourceTypesImpl.conforms[A, B]
  def echo[A](value: A): A = macro SourceTypesImpl.echo[A]
}
object Main {
  def abstractType[T <: CharSequence](value: T): T = {
    println(SourceTypes.shape[T])
    println(SourceTypes.conforms[T, CharSequence])
    SourceTypes.echo[T](value)
  }
  def main(args: Array[String]): Unit = {
    println(SourceTypes.shape[LocalBox[String]])
    println(SourceTypes.shape[List[LocalBox[String]]])
    println(SourceTypes.inspect[LocalBox[String]])
    println(SourceTypes.inspect[LocalBox[java.lang.StringBuilder]])
    println(SourceTypes.conforms[LocalBox[String], Parent[String]])
    println(SourceTypes.conforms[LocalBox[String], LocalBox[CharSequence]])
    println(SourceTypes.conforms[LocalBox[String], LocalBox[java.lang.StringBuilder]])
    println(SourceTypes.echo[LocalBox[String]](new LocalBox("roundtrip")).value)
    println(abstractType[String]("abstract"))
  }
}
