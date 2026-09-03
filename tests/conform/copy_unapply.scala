// case class `copy`, default arguments, destructuring in patterns and vals,
// and a hand-written `unapply` / `unapplySeq`.
object Main {
  case class Column(name: String, tpe: String = "INT", nullable: Boolean = false, default: Option[String] = None) {
    def ddl: String = name + " " + tpe + (if (nullable) "" else " NOT NULL") + default.map(" DEFAULT " + _).getOrElse("")
  }

  object Prefixed {
    def unapply(s: String): Option[(String, String)] = {
      val i = s.indexOf('.')
      if (i < 0) None else Some((s.substring(0, i), s.substring(i + 1)))
    }
  }

  object Words {
    def unapplySeq(s: String): Option[Seq[String]] =
      if (s.isEmpty) None else Some(s.split(" ").toSeq)
  }

  case class Range2(lo: Int, hi: Int)
  object Between {
    def unapply(r: Range2): Option[(Int, Int)] = if (r.lo <= r.hi) Some((r.lo, r.hi)) else None
  }

  def describe(s: String): String = s match {
    case Prefixed(t, c) => s"table=$t col=$c"
    case Words(one) => s"one word: $one"
    case Words(a, b, _*) => s"starts $a/$b"
    case _ => "empty"
  }

  def main(args: Array[String]): Unit = {
    val base = Column("id")
    println(base.ddl)
    println(base.copy(nullable = true).ddl)
    println(base.copy(tpe = "TEXT", default = Some("''")).ddl)
    println(Column("n", nullable = true).ddl)
    println(Column(default = Some("0"), name = "k").ddl)

    val Column(n, t, _, _) = base.copy(tpe = "BIGINT")
    println(n + "/" + t)

    println(describe("users.id"))
    println(describe("hello"))
    println(describe("a b c"))
    println(describe(""))

    println(Range2(1, 5) match { case Between(a, b) => b - a; case _ => -1 })
    println(Range2(5, 1) match { case Between(a, b) => b - a; case _ => -1 })

    val cols = List(base, base.copy(name = "name", tpe = "TEXT"), Column("age", nullable = true))
    println(cols.map(_.ddl).mkString("; "))
    println(cols.map { case Column(nm, tp, nu, _) => s"$nm:$tp:$nu" })
    println(base == Column("id", "INT", false, None))
    println(base.productArity + " " + base.productPrefix + " " + base.productElement(1))
    println(base.copy(name = "id2").hashCode == Column("id2").hashCode)
    val (x, y) :: rest = List((1, "a"), (2, "b"))
    println(s"$x$y ${rest.length}")
  }
}
