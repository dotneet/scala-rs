// nsc: "macro implementation not found ... the most common reason for that is
// that you cannot use macro implementations in the same compilation run that
// defines them". Expanding a macro means *loading* the implementation's class
// file, and this run has not written one yet.
//
// scala-rs says the same, and says it as a reason on the macro diagnostic
// rather than accepting the call: a macro def has no bytecode, so a silent
// pass would emit a call to a method that is not there.
import scala.reflect.macros.blackbox.Context
import scala.language.experimental.macros

object EgSameRun {
  def implF(c: Context)(): c.Tree = {
    import c.universe._
    Literal(Constant(1))
  }
}

object EgSameRunUse {
  def f(): Int = macro EgSameRun.implF
}

object Main {
  def main(args: Array[String]): Unit = println(EgSameRunUse.f())
}
