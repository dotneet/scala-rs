// Type arguments on a macro implementation reference that scala-rs refuses,
// each with a reason that names the missing capability. `docs/macros.md` §7.22.
//
// **Real scalac 2.13.16 compiles and runs this whole file**, printing
//
//     R=List[A] U=Int
//     R=List[A] U=Int
//     R=Boolean U=U
//
// so nothing here is a program nsc rejects. scala-rs accepts none of the three,
// which is the half that keeps `mt2_use.scala` honest: a bridge that resolved
// these by substituting would print `List[String]` and `Int` where nsc prints
// `List[A]` and `U`, and no test would notice.
import mt2._
import scala.language.experimental.macros

// The same refusal reached through a macro def **this run compiles**, so both
// readers of the implementation reference are covered: `Awkward.listOf` below
// comes out of the pickle.
object LocalAwkward {
  def listOf[R]: String = macro Mt2Impl.pairImpl[List[R], Int]
}

// A macro called with no receiver at all. nsc reads a type parameter of the
// macro def's owner off the prefix, and with no prefix it uses the owner's own
// type -- so `U` reaches the implementation as the free type parameter `U`.
class LocalInner[U](val u: U) {
  def inside[R]: String = macro Mt2Impl.pairImpl[R, U]
  def viaThis: String = inside[Boolean]
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Awkward.listOf[String])
    println(LocalAwkward.listOf[String])
    println(new LocalInner[Int](1).viaThis)
  }
}
