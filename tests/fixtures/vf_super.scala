// `super.m` must resolve in the parent linearization, whatever an *earlier*
// selection of the same overload head happened to record.
//
// slick declares `def expr(n: Node)` on seven `QueryBuilder` subclasses next
// to the inherited `final def expr(n: Node, skipParens: Boolean)`. The
// unqualified `expr(c, true)` inside one subclass recorded the overload group
// `[QB.expr(String, Boolean), OracleQB.expr(String)]` under the two-parameter
// head, and `overload_groups` is keyed by that symbol alone -- so the *next*
// class's `super.expr(n)` was resolved against the previous class's
// alternatives. `PostgresQueryBuilder` emitted
// `invokespecial OracleProfile$OracleQueryBuilder.expr` and the JVM refused
// the class: `VerifyError: Bad invokespecial instruction: current class isn't
// assignable to reference class`. Four more profiles got their *own* `expr`
// back, which verifies fine and recurses for ever.
package vf

trait Comp {
  class QB(val t: String) {
    final def expr(n: String, skipParens: Boolean): Unit =
      println("base2 " + n + " " + skipParens)
    def expr(n: String): Unit = println("base " + n)
  }
}

trait OracleP extends Comp {
  class OracleQB(t: String) extends QB(t) {
    override def expr(c: String): Unit = c match {
      case "o" => println("O")
      // The unqualified call that records the cross-owner group.
      case "p" => expr(c, true)
      case _   => super.expr(c)
    }
  }
}

trait PostgresP extends Comp {
  class PostgresQB(t: String) extends QB(t) {
    // No declared result type, exactly as slick writes it.
    override def expr(n: String) = n match {
      case "g" => println("G")
      case _   => super.expr(n)
    }
  }
}

// A third one, so the "resolved to its own override" shape (infinite
// recursion) is pinned as well as the cross-class one.
trait MysqlP extends Comp {
  class MysqlQB(t: String) extends QB(t) {
    override def expr(n: String): Unit = n match {
      case "m" => println("M")
      case "q" => expr(n, false)
      case _   => super.expr(n)
    }
  }
}

object Main extends PostgresP with OracleP with MysqlP {
  def main(args: Array[String]): Unit = {
    new PostgresQB("x").expr("g")
    new PostgresQB("x").expr("z")
    new OracleQB("x").expr("o")
    new OracleQB("x").expr("p")
    new OracleQB("x").expr("z")
    new MysqlQB("x").expr("m")
    new MysqlQB("x").expr("q")
    new MysqlQB("x").expr("z")
    new QB("x").expr("plain")
    new QB("x").expr("plain", true)
  }
}
