package regression {
  /** Compiled before the call site so the use fixture sees this as a binary type. */
  final case class BinaryValue(member: Int)
}

package org.scalatest {
  import scala.language.experimental.macros
  import scala.reflect.macros.whitebox.Context

  /** A small ScalaTest-shaped assertion helper: inspect then retype the Expr. */
  object BinaryAssertionMacro {
    def retypeSelected(c: Context)(value: c.Expr[Int]): c.Expr[Int] = {
      import c.universe._
      value.tree match {
        case selected @ Select(qualifier, _) if qualifier.symbol != NoSymbol =>
          c.Expr[Int](c.untypecheck(selected.duplicate))
        case other =>
          c.abort(other.pos, "expected a selected binary member")
      }
    }
  }

  object BinaryAssertions {
    def retypeSelected(value: Int): Int = macro BinaryAssertionMacro.retypeSelected
  }
}
