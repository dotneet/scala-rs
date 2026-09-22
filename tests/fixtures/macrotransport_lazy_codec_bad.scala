import macrotransport.lazycodec.Codec
import shapeless.Lazy

object Main {
  val missing: Lazy[Codec[String]] = implicitly[Lazy[Codec[String]]]
}
