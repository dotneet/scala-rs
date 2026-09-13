package org.scalactic {
  import scala.reflect.macros.whitebox.Context

  final class MacroOwnerRepair[C <: Context](val c: C) {
    def repairOwners[A](expr: c.Expr[A]): c.Expr[A] =
      throw new IllegalStateException("nsc-only owner repair was invoked")
  }
}

package org.scalatest {
  import scala.language.experimental.macros
  import scala.reflect.macros.whitebox.Context

  object AssertionsMacro {
    def identity(c: Context)(value: c.Expr[Int]): c.Expr[Int] = {
      val repair = new org.scalactic.MacroOwnerRepair[c.type](c)
      repair.repairOwners(value)
    }

    def withClue(c: Context)(value: c.Expr[Int], clue: c.Expr[Any]): c.Expr[Int] = {
      val repair = new org.scalactic.MacroOwnerRepair[c.type](c)
      repair.repairOwners(value)
    }

    def typedMatch(c: Context)(): c.Expr[Int] = {
      import c.universe._
      val repair = new org.scalactic.MacroOwnerRepair[c.type](c)
      val tree = c.typecheck(q"(1, 2) match { case (a, b) => a + b }")
      repair.repairOwners(c.Expr[Int](tree))
    }

    def retype(c: Context)(value: c.Expr[Int]): c.Expr[Int] = {
      val repair = new org.scalactic.MacroOwnerRepair[c.type](c)
      repair.repairOwners(c.Expr[Int](c.untypecheck(value.tree.duplicate)))
    }

    def retypeArray(c: Context)(value: c.Expr[Array[String]]): c.Expr[Array[String]] = {
      val repair = new org.scalactic.MacroOwnerRepair[c.type](c)
      repair.repairOwners(c.Expr[Array[String]](c.untypecheck(value.tree.duplicate)))
    }
  }

  object Assertions {
    def identity(value: Int): Int = macro AssertionsMacro.identity
    def withClue(value: Int, clue: Any): Int = macro AssertionsMacro.withClue
    def typedMatch(): Int = macro AssertionsMacro.typedMatch
    def retype(value: Int): Int = macro AssertionsMacro.retype
    def retypeArray(value: Array[String]): Array[String] = macro AssertionsMacro.retypeArray
  }
}
