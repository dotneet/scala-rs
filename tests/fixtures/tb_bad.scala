// `staticClass` / `staticModule` / `staticPackage` are declared by the
// abstract *class* `scala.reflect.api.Mirror[U <: Universe with Singleton]`,
// and a `JavaUniverse`'s mirror reaches them through the parent
// `Mirror[JavaUniverse.this.type]`.
//
// This file was written as a confession: that parent did not convert, so
// `api.Mirror` was not in the mirror's linearisation and scala-rs reported a
// diagnostic for every line while real scalac 2.13.16 compiled and ran it.
// `agent/backendtypes` closed it while making a concrete class's type members
// resolve to the definitions that fix them, and the output now matches
// scalac's byte for byte (`expected/tb_bad.txt`).
//
// `classSymbol` and `moduleSymbol` (`tb_reflect.scala`) are declared by
// `Mirrors.RuntimeMirror`, an ordinary trait, and were never affected.
import scala.reflect.runtime.{currentMirror => cm}

object TbBadCompanion

object Main {
  def main(args: Array[String]): Unit = {
    println(cm.staticClass("java.lang.String"))
    println(cm.staticModule("TbBadCompanion"))
    println(cm.staticPackage("scala"))
  }
}
