// Applied source classes and weak type parameters are supported. The final
// source singleton tag still fails with a specific diagnostic.
import scala.language.experimental.macros
import scala.reflect.ClassTag

class LocalBox[A](val a: A)

object GbmBadUse {
  def caseInfo[R](implicit ct: ClassTag[R]): String = macro GbmImpl.caseInfoImpl[R]
  def shape[A]: String = macro GbmImpl.shapeImpl[A]
}

object Main {
  // A weak type keeps the original method type parameter identity.
  def viaTypeParam[T]: String = GbmBadUse.shape[T]

  def main(args: Array[String]): Unit = {
    println(GbmBadUse.shape[LocalBox[Int]])
    println(viaTypeParam[Int])
    println(GbmBadUse.shape[Main.type])
  }
}
