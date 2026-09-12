// A context bound beside an explicitly written implicit clause. nsc desugars the
// bound into that *same* clause, with the synthesized evidence first:
//   def a[E: Sem](implicit P: Par[List])  =>  a(Sem[E], Par[List])
// (`javap -p -s` on scalac 2.13.16). Emitting it as a second implicit clause left
// the method with a shape no Scala 2 source can write, and the call site filled
// only the first -- a one-argument call against a two-parameter method, i.e.
// `VerifyError: Operand stack underflow` on the first execution. cats'
// `ParallelInstances.catsParallelForEitherTNestedParallelValidated` is this
// shape, and its whole class failed to load.
trait Sem[A] { def tag: String }
trait Ord2[A] { def tag: String }
trait Par[M[_]] { def tag: String }

object O {
  def a[E: Sem](implicit P: Par[List]): String = "a:" + P.tag + "," + implicitly[Sem[E]].tag
  def b[E](implicit P: Par[List], e: Sem[E]): String = "b:" + P.tag + "," + e.tag
  def c[E: Sem]: String = "c:" + implicitly[Sem[E]].tag
  def d[E: Sem: Ord2]: String = "d:" + implicitly[Sem[E]].tag + "," + implicitly[Ord2[E]].tag
  def e[E: Sem: Ord2](implicit P: Par[List]): String =
    "e:" + P.tag + "," + implicitly[Sem[E]].tag + "," + implicitly[Ord2[E]].tag
  def f[M[_], E: Sem](implicit P: Par[M]): String = g[M, E]
  def g[M[_], E: Sem](implicit P: Par[M]): String = "g:" + P.tag + "," + implicitly[Sem[E]].tag
}

object Main {
  implicit val s: Sem[Int] = new Sem[Int] { def tag = "SemInt" }
  implicit val o: Ord2[Int] = new Ord2[Int] { def tag = "OrdInt" }
  implicit val p: Par[List] = new Par[List] { def tag = "ParList" }
  def main(args: Array[String]): Unit = {
    println(O.a[Int])
    println(O.b[Int])
    println(O.c[Int])
    println(O.d[Int])
    println(O.e[Int])
    println(O.f[List, Int])
  }
}
