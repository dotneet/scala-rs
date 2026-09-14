// `ClassTag.apply(classOf[T])` is a compiler intrinsic only for
// scala.reflect.ClassTag.  A source-level companion with the same short name
// must remain an ordinary method call.
object ClassTag {
  def apply(c: Any): String = "custom:" + c
}

object Main {
  def main(args: Array[String]): Unit =
    println(ClassTag.apply(classOf[String]))
}
