// `currentMirror` is one of nsc's *fast-track* macros (`scala.tools.reflect
// .FastTrack`): the compiler recognises it by its full name and never even
// consults the (placeholder) `@macroImpl` annotation on the classfile.
// scala-rs expands it the same way, in `crates/typer/src/fasttrack_mirror.rs`.
//
// This file used to be `rt_currentmirror_bad.scala` -- a confession that the
// name was visible but no reference to it could be expanded. It is now an
// ordinary fixture: what it prints has to match real scalac 2.13.16.
//
// Both lines are chosen to be stable across machines. `println(currentMirror)`
// is not: a mirror's `toString` carries the class loader's identity hash.
import scala.reflect.runtime.universe._
import scala.reflect.runtime.currentMirror

object Main {
  def main(args: Array[String]): Unit = {
    // The expansion is `runtimeMirror(this.getClass.getClassLoader)`, and a
    // universe hands out one mirror per class loader.
    println(currentMirror == runtimeMirror(getClass.getClassLoader))
    println(currentMirror.classSymbol(classOf[String]).fullName)
  }
}
