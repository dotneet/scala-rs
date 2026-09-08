// Call sites for the applied-type-constructor tags of `gbm_impl.scala`.
// `docs/macros.md` §7.21.
//
// Real scalac 2.13.16 compiles this file against the same implementations and
// `crates/cli/tests/gbmapto.rs` compares the two programs' output line for
// line. A tag that carried the type constructor without its arguments -- or
// with the wrong ones -- would still compile and still run here; only the
// output would differ.
import scala.language.experimental.macros
import scala.reflect.ClassTag

object GbmUse {
  // The `mapTo` shape: an implicit `ClassTag[R]` the compiler materialises,
  // whose type is what the request has to carry.
  def caseInfo[R](implicit ct: ClassTag[R]): String = macro GbmImpl.caseInfoImpl[R]
  def shape[A]: String = macro GbmImpl.shapeImpl[A]
  def conforms[A, B]: String = macro GbmImpl.conformsImpl[A, B]
}

object Main {
  def main(args: Array[String]): Unit = {
    println(GbmUse.caseInfo[GbmRow])
    println(GbmUse.shape[List[Int]])
    // `Either` is `scala.package.Either` and `Map` is `Predef.Map`: both are
    // *aliases*. scala-rs expands aliases away before a type reaches the tag
    // descriptor, so its tag names the class where nsc's names the alias --
    // the same type, printed two ways, because nsc omits the prefix of an
    // alias owned by `scala` or `Predef` and does not omit `scala.util.` or
    // `scala.collection.immutable.`. `crates/cli/tests/gbmapto.rs` pins both
    // spellings rather than letting the difference go unnoticed.
    println(GbmUse.shape[Either[String, List[Int]]])
    println(GbmUse.shape[Map[String, List[Int]]])
    println(GbmUse.shape[(Int, String)])
    println(GbmUse.shape[Int => String])
    println(GbmUse.shape[Array[Int]])
    println(GbmUse.shape[ClassTag[GbmRow]])
    println(GbmUse.conforms[List[Int], Seq[Int]])
    println(GbmUse.conforms[Seq[Int], List[Int]])
  }
}
