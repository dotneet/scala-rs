// Call sites for the `c.typecheck` macros of `mtc_impl.scala`.
// `docs/macros.md` §7.20.
//
// Real scalac 2.13.16 compiles this file the same way against the same
// implementations, and `crates/cli/tests/macromirror.rs` compares the two
// programs' output line for line. A `c.typecheck` that answered something
// other than what the compiler really thinks would still compile and still
// run; only the output would differ.
import scala.language.experimental.macros

// A class this compilation run is defining. It has no class file while this
// file is being compiled, so the macro engine's own mirror can never find it:
// every question about it has to be answered by scala-rs.
class Marker {
  def label: String = "marker"
}

// A class of the same kind whose declaration mentions itself.
class Node {
  def next: Node = this
}

object MtcUse {
  def constType(): String = macro MtcImpl.constTypeImpl
  def typeOfName(): String = macro MtcImpl.typeOfNameImpl
  def runClassType(): String = macro MtcImpl.runClassTypeImpl
  def memberType(): String = macro MtcImpl.memberTypeImpl
  def typeMode(): String = macro MtcImpl.typeModeImpl
  def probe(): String = macro MtcImpl.probeImpl
  def selfRef(): String = macro MtcImpl.selfRefImpl
  def echo(): Int = macro MtcImpl.echoImpl
}

object Main {
  // Both are in scope at every call site below, and neither is visible to the
  // macro implementation, which was compiled before this file existed.
  def secondsInAnHour: Int = 3600
  def origin: Marker = new Marker
  def aNode: Node = new Node

  def main(args: Array[String]): Unit = {
    println(MtcUse.constType())
    println(MtcUse.typeOfName())
    println(MtcUse.runClassType())
    println(MtcUse.memberType())
    println(MtcUse.typeMode())
    println(MtcUse.probe())
    println(MtcUse.selfRef())
    println(MtcUse.echo())
  }
}
