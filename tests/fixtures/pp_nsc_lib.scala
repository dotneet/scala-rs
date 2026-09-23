// Compiled by real scalac. scala-rs reads its `ScalaSignature` back in
// `pp_ours.scala`: a `MethodType` over an empty list has to arrive as one.
case class NsEmpty()

object NsTaggedFactory {
  def make[T: scala.reflect.ClassTag](): String =
    implicitly[scala.reflect.ClassTag[T]].runtimeClass.getSimpleName
}

trait NsTicker {
  def tick(): Int
  def label: String
}

object NsKeys extends NsTicker {
  def tick(): Int = 3
  def label: String = "nsc"
  def unit(): Unit = ()
  def withImplicit()(implicit ord: Ordering[Int]): Int = ord.compare(2, 1)
  def curried(a: Int)(b: Int): Int = a * b
}
