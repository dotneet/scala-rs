// A confession, not a test of correct behaviour: real scalac 2.13.16 compiles
// and runs this, and scala-rs reports a diagnostic for every line.
//
// `staticClass` / `staticModule` / `staticPackage` are declared by the
// abstract *class* `scala.reflect.api.Mirror[U <: Universe with Singleton]`,
// and a `JavaUniverse`'s mirror reaches them through the parent
// `Mirror[JavaUniverse.this.type]`. That parent does not convert -- the
// argument is a singleton type of the enclosing class, which
// `PickleSupply::conv_at` has no reading for -- so `api.Mirror` is not in the
// mirror's linearisation at all and none of its members are reachable.
// `classSymbol` and `moduleSymbol` (`tb_reflect.scala`) are declared by
// `Mirrors.RuntimeMirror`, an ordinary trait, and are unaffected.
//
// Fixing this means converting a `this.type` parent, which is a change to the
// parent reader and not to the `RuntimeClass` erasure this slice is about.
import scala.reflect.runtime.{currentMirror => cm}

object TbBadCompanion

object Main {
  def main(args: Array[String]): Unit = {
    println(cm.staticClass("java.lang.String"))
    println(cm.staticModule("TbBadCompanion"))
    println(cm.staticPackage("scala"))
  }
}
