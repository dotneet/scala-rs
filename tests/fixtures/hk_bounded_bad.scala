trait Bound
trait M { type F[_] <: Bound }
class C extends M { type F[X] = Int }
object Main {
  def main(args: Array[String]): Unit = ()
}
