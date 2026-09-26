// The parents a case class may be given synthetically -- `Product`,
// `ProductN`, `Serializable` -- can be written twice: nsc drops the
// repetition (`fixDuplicateSyntheticParents`) before it checks for twins.
trait T0 { def a0: Int = 7 }
class K1 extends T0 with java.io.Serializable with java.io.Serializable
class K2 extends T0 with Serializable with Serializable
class K3 extends T0 with Product with Product {
  def canEqual(x: Any) = true; def productArity = 0; def productElement(n: Int) = ???
}
class K4 extends T0 with scala.Serializable with java.io.Serializable
class K6 extends T0 with Product1[Int] with Product1[Int] { def canEqual(x: Any) = true; def _1 = 1 }
case class K7(i: Int) extends Product with Serializable with Serializable
object Main {
  def main(args: Array[String]): Unit = {
    println(List(new K1().a0, new K2().a0, new K3().productArity, new K4().a0, new K6()._1, K7(3).i))
    println(List(new K1, new K2, K7(3)).map(_.isInstanceOf[java.io.Serializable]))
  }
}
