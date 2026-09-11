// Type tags scala-rs cannot build, each refused with a reason that names what
// was missing. `docs/macros.md` §7.21.
//
// Real scalac 2.13.16 compiles and runs this file -- it prints
//
//     LocalBox[Int] = LocalBox[Int]
//     T = T[]
//     Main.type = Main.type[]
//
// -- and scala-rs answers none of the three. That is the point of the file: a
// tag that was guessed at would compile here and be *wrong*, and the macro
// would build its tree out of a type nobody asked about.
//
// A fourth call used to be here: `caseInfo[LocalRow]` for a case class this
// run is compiling, gitbucket's `mapTo` in miniature, refused because the
// class travelled as an empty placeholder. It now travels as its identity and
// is described in nsc's shape when asked (`docs/macros.md` §7.25), so it
// moved to `tests/fixtures/gbmac_caseinfo_use.scala`, which prints what real
// scalac prints.
import scala.language.experimental.macros
import scala.reflect.ClassTag

class LocalBox[A](val a: A)

object GbmBadUse {
  def caseInfo[R](implicit ct: ClassTag[R]): String = macro GbmImpl.caseInfoImpl[R]
  def shape[A]: String = macro GbmImpl.shapeImpl[A]
}

object Main {
  // A tag for a bare type parameter. nsc materialises a `WeakTypeTag` with a
  // free type here; scala-rs has no such thing and says so.
  def viaTypeParam[T]: String = GbmBadUse.shape[T]

  def main(args: Array[String]): Unit = {
    println(GbmBadUse.shape[LocalBox[Int]])
    println(viaTypeParam[Int])
    println(GbmBadUse.shape[Main.type])
  }
}
