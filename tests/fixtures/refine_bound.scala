trait Bound
class Id[A](val value: A) extends Bound
trait Box { type A; def x: A }
class IntBox extends Box { type A = Int; def x: A = 41 }
trait M { type F[_] <: Bound; def wrap(x: Int): F[Int] }
class C extends M {
  type F[X] = Id[X]
  def wrap(x: Int): F[Int] = new Id(x)
}
object Main {
  def get(b: Box { type A <: Int }): Int = b.x
  def main(args: Array[String]): Unit = {
    println(get(new IntBox))
    println(new C().wrap(2).value)
  }
}
