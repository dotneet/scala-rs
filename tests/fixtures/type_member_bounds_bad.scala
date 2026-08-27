trait Bound
class C { type A <: Bound }
class D extends C { type A = Int }
class Lo { type A >: String }
class LoBad extends Lo { type A = Int }
object Main {
  def main(args: Array[String]): Unit = ()
}
