// The library half of the "defaults declared in a class file" test. Compiled
// separately, by **real scalac**, so the consumer sees only the class files
// and nsc's own pickle and never this source. That is the setting the defect
// appears in: gitbucket calls json4s and scalatra out of published jars.
package dalib

import scala.reflect.ClassTag

// json4s's `FieldSerializer` shape: a case class whose companion `apply` is
// synthetic, has a default for every explicit parameter, and ends in an
// implicit clause. The class file records neither the defaults nor the
// implicit clause.
case class Box[A](tag: String = "t", size: Int = 7, deep: Boolean = true)(implicit
    ct: ClassTag[A]
) {
  def show: String = tag + "/" + size + "/" + deep + "/" + ct.runtimeClass.getSimpleName
}

// An ordinary object method whose later clause's default reads earlier ones.
// The pickled getter stays curried (`join$default$3(a)(b)`) though its class
// file method takes both parameters at once.
object Plain {
  def join(a: String)(b: String = "-")(c: String = a + b): String = a + b + c
}
