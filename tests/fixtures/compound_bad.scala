// `A with B` is a legal *type* even where no value can inhabit it (scalac
// accepts the signature); what it is not is a way to reach a member neither
// parent declares.
trait A { def a: Int }
trait B { def b: Int }
object Main {
  def bad(x: A with B): Int = x.c
  def main(args: Array[String]): Unit = ()
}
