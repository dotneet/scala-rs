// Overloaded references, constructors and annotations that scalac 2.13.16
// refuses while typing; scala-rs compiled every one of them.
class X(x: Int)
class Two(a: Int)(b: Int)
class Holder { def over(a: Int): Int = a; def over(a: String): Int = 1 }
object Test {
  def over(a: Int): Int = a
  def over(a: String): Int = 1
  val o = over
  def any(a: Any): Int = 1
  def any(a: String): Int = 2
  val m = any
  val sel = new Holder().over
  val f: String => Int = _.length
  def f(s: String): Int = 2
  val called = f("")
  val n1 = new X
  val n2 = new Two
  @NoSuchAnnot def annotated = 1
  @annotation.xxxxx def qualified = 2
  @throws(classOf[Missing]) def thrown = 3
}
