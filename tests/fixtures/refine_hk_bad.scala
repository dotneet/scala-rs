trait M { type F[_] }
class C extends M { type F[X] = List[X] }
object Main {
  def asProper(m: M { type F[_] })(x: m.F): Unit = ()
  def main(args: Array[String]): Unit = ()
}
