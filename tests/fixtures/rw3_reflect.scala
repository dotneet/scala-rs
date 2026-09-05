// What runtime reflection can read back out of the `ScalaSignature` we write.
//
// Two things this pins down, both of which our pickle used to get wrong and
// neither of which any other check can see -- the classfile loads, verifies
// and lints either way:
//
//   * a constructor's result type carries the *prefix* an instance is reached
//     through, so `class A` inside `object Main` is `def <init>(): Main.A` and
//     the same class inside `class Outer` is `def <init>(): Outer.this.A`.
//     With no prefix nsc printed a bare `A`.
//   * an `object`'s module class has a primary constructor. nsc's namer gives
//     every one of them a `private Main$BB$()`, and reflection looks it up as
//     `termNames.CONSTRUCTOR`; our pickle listed only the nested members.
import scala.reflect.runtime.{currentMirror => cm}

object Main {
  class A { def foo = 1 }

  object BB {
    class B1
  }

  def main(args: Array[String]): Unit = {
    println(cm.classSymbol(classOf[A]).info)
    println(cm.classSymbol(classOf[Outer#B]).info)
    val bb = cm.moduleSymbol(BB.getClass)
    println(bb.info.decls.toList)
  }
}

class Outer {
  class B { def bar = 2 }
}
