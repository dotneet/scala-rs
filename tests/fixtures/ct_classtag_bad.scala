// The refusing half of `ct_classtag.scala`. Every line is rejected by scalac
// 2.13.16 with the message written beside it; before this rule scala-rs
// compiled all six without a word, and answered `classTag[T]` with a tag for
// whatever `T` erased to.
import scala.reflect.{ClassTag, classTag}

trait HasMember {
  type E
  // No ClassTag available for HasMember.this.E
  def member: Unit = println(classTag[E])
}

class Holder[A] {
  // No ClassTag available for A
  def ofClassParam: Unit = println(classTag[A])
}

object Main {
  // No ClassTag available for T
  def bare[T]: Unit = println(classTag[T])

  // An upper bound is not a tag either.
  // No ClassTag available for T
  def bounded[T <: String]: Unit = println(classTag[T])

  // No ClassTag available for T
  def summoned[T]: Unit = println(implicitly[ClassTag[T]])

  // An array of an untagged element is untagged.
  // No ClassTag available for Array[T]
  def arrayOf[T]: Unit = println(classTag[Array[T]])

  // nsc words this one differently -- it is `typedNew`'s own check.
  // cannot find class tag for element type T
  def built[T]: Array[T] = new Array[T](3)

  def main(args: Array[String]): Unit = bare[Int]
}
