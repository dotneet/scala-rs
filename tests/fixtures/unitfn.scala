trait UnitBase[A] { def call(f: A => Int): Int }
class UnitImpl extends UnitBase[Unit] {
  override def call(f: Unit => Int): Int = f(())
}
object Main {
  type Callback[A] = Unit => A
  def invoke[A](f: Callback[A]): A = f(())
  def main(args: Array[String]): Unit = {
    val one: Unit => Int = (_: Unit) => 7
    val parenthesized: (Unit) => Int = (_: Unit) => 8
    val zero: () => Int = () => 9
    println(invoke(one))
    println(parenthesized(()))
    println(zero())
    println(new UnitImpl().call(one))
    def delayed(n: Int): Unit => Int = (_: Unit) => n + 1
    println(delayed(1)(()))
  }
}
