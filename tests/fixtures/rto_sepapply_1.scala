// Compiled first; `rto_sepapply_2.scala` sees it only as a classfile.
trait Companion[T] {
  def parse(value: String): Option[T]
  def apply(value: String): T = parse(value).getOrElse(throw new IllegalArgumentException(value))
}
