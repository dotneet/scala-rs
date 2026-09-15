import scala.language.experimental.macros
class LocalBox[A <: CharSequence](val value: A)
object SourceTypes {
  def echo[A](value: A): A = macro SourceTypesImpl.echo[A]
}
object Main {
  val wrong: LocalBox[java.lang.StringBuilder] = SourceTypes.echo[LocalBox[String]](new LocalBox("bad"))
}
