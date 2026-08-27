trait M { type F[_] }
class C extends M { type F = Int }
object Main {
  def asProper(m: M)(x: m.F): Unit = ()
  def main(args: Array[String]): Unit = ()
}
