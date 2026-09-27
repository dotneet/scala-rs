// A shapeless derivation: `generic` needs the `Generic.Aux` macro, `hcons` a
// `Lazy` macro, both of which nsc runs with its own scala-reflect and
// scala-compiler. Compiled with only shapeless on the classpath, as a scalac
// user writes it.
package sg
import shapeless._
trait Enc[A] { def enc(a: A): String }
object Enc {
  def apply[A](implicit e: Enc[A]): Enc[A] = e
  implicit val int: Enc[Int] = (a: Int) => a.toString
  implicit val str: Enc[String] = (a: String) => "'" + a + "'"
  implicit val dbl: Enc[Double] = (a: Double) => a.toString
  implicit def list[A](implicit e: Enc[A]): Enc[List[A]] = (a: List[A]) => a.map(e.enc).mkString("[", ",", "]")
  implicit val hnil: Enc[HNil] = (_: HNil) => ""
  implicit def hcons[H, T <: HList](implicit h: Lazy[Enc[H]], t: Enc[T]): Enc[H :: T] = (a: H :: T) => h.value.enc(a.head) + ";" + t.enc(a.tail)
  implicit def generic[A, R](implicit g: Generic.Aux[A, R], r: Lazy[Enc[R]]): Enc[A] = (a: A) => "{" + r.value.enc(g.to(a)) + "}"
}
object G0 {
  final case class R0(f0: Int, f1: String, f2: Double, f3: List[Int], f4: List[String], f5: Int)
  val e0 = Enc[R0]
}
object Main { def main(args: Array[String]): Unit = println(G0.e0.enc(G0.R0(1, "a", 2.0, List(1), List("x"), 3))) }
