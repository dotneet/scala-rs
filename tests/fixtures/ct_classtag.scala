// A `ClassTag` is *built* out of the erasure of the type it tags, so every
// shape whose erasure is a class has one without anything being in scope --
// and an abstract type has one only when the scope supplies it. This is the
// accepting half of that rule; `ct_classtag_bad.scala` is the refusing half.
//
// Every line here was checked against scalac 2.13.16, which accepts the file
// and prints the same output.
import scala.reflect.{ClassTag, classTag}

class Cell[A](val a: A)

object Main {
  // A class, whatever its type arguments: the tag is `classOf[C]`, and the
  // arguments are erased away rather than looked for.
  def named[T]: String = classTag[List[T]].runtimeClass.getName

  // A context bound is the scope an abstract type's tag comes from.
  def bound[T: ClassTag]: String = classTag[T].runtimeClass.getName

  // ... and it reaches through `Array`, however deep. nsc does not build a
  // `classOf` of the array type here: it wraps the *element*'s tag, so this
  // has to report `[[I` at `T = Int` and not the element's `int`.
  def nested[T: ClassTag]: String =
    implicitly[ClassTag[Array[Array[T]]]].runtimeClass.getName
  def mkArray[T: ClassTag](n: Int): Array[T] = new Array[T](n)
  def dim[T: ClassTag]: String = Array.ofDim[T](2, 3).getClass.getName

  // An intersection erases to its dominator, which prefers a parent that is a
  // class -- so `T with AnyRef` is tagged even though `T` alone is not.
  def refined[T]: String = classTag[T with AnyRef].runtimeClass.getName

  // A singleton widens to the class it is an instance of.
  def singleton: String = classTag[Main.type].runtimeClass.getName.take(4)

  def main(args: Array[String]): Unit = {
    println(named[Int])
    println(bound[String])
    println(classTag[Int].runtimeClass.getName)
    println(nested[Int])
    println(mkArray[String](2).length)
    println(dim[Int])
    println(refined[Cell[Int]])
    println(singleton)
    println(implicitly[ClassTag[Array[Int]]].runtimeClass.getName)
  }
}
