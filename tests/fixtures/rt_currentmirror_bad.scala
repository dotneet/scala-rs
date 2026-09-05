// `currentMirror` is one of nsc's *fast-track* macros (`scala.tools.reflect
// .FastTrack`): the compiler recognises it by its full name and never even
// consults the (placeholder) `@macroImpl` annotation on the classfile. Real
// scalac accepts and runs this file; scala-rs makes the name visible (it is a
// real, pickled macro def) but does not yet expand it, so this is a
// confession of what remains unimplemented, not a rejection of something
// that should be rejected.
import scala.reflect.runtime.universe._
import scala.reflect.runtime.currentMirror

object Main {
  def main(args: Array[String]): Unit = {
    println(currentMirror)
  }
}
