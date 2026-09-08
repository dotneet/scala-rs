// Type tags scala-rs cannot build, each refused with a reason that names what
// was missing. `docs/macros.md` §7.21.
//
// Real scalac 2.13.16 compiles and runs this file -- it prints
//
//     Nothing v:Int
//     LocalBox[Int] = LocalBox[Int]
//     T = T[]
//     Main.type = Main.type[]
//
// -- and scala-rs answers none of the four. That is the point of the file: a
// tag that was guessed at would compile here and be *wrong*, and the macro
// would build its tree out of a type nobody asked about.
//
// The first one is gitbucket's `mapTo` in miniature. `LocalRow` is a case
// class **this run is compiling**, so it has no class file for the engine's
// mirror to find and it travels as the empty placeholder of `docs/macros.md`
// §5.1. The implementation asks it `isCaseClass`, is told `false` by a symbol
// carrying nothing but a name, and aborts -- and that verdict says nothing
// about this program, so it is replaced rather than repeated.
import scala.language.experimental.macros
import scala.reflect.ClassTag

case class LocalRow(v: Int)
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
    println(GbmBadUse.caseInfo[LocalRow])
    println(GbmBadUse.shape[LocalBox[Int]])
    println(viaTypeParam[Int])
    println(GbmBadUse.shape[Main.type])
  }
}
