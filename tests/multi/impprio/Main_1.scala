// SLS 2, "Identifiers, Names and Scopes": a binding of higher precedence
// *hides* one of lower precedence in the same scope. Each package below is one
// arrangement, and each prints which definition the name was bound to -- the
// only way to tell, since every one of them compiles either way.
//
// The expected output is real scalac 2.13.16's, for this same source.

// (2) explicit import over (3) wildcard import, wildcard written first.
package p1 {
  import lib.Wild._
  import lib.Exp.name
  object Case { def who: String = name }
}

// The same, with the wildcard written *last*: precedence is not insertion
// order. This is gitbucket's arrangement upside down.
package p2 {
  import lib.Exp.name
  import lib.Wild._
  object Case { def who: String = name }
}

// A renaming selector is an explicit import too.
package p3 {
  import lib.Wild._
  import lib.Exp.{other => name}
  object Case { def who: String = name }
}

// The type namespace is ranked separately but by the same rule.
package p4 {
  import lib.Wild._
  import lib.Exp.T
  object Case { def who: String = new T().who }
}

// (4) a package member from ANOTHER compilation unit (`Sib_1.scala`) loses to
// a wildcard import.
package p5 {
  import lib.Wild._
  object Case { def who: String = Sibling.who }
}

// (1) a package member from THIS compilation unit beats a wildcard import.
// gitbucket's `servlet/TransactionFilter.scala` is exactly this: `object
// Database` at the bottom of the file whose `import … blockingApi._` is at
// the top.
package p6 {
  import lib.Wild._
  object Shared { def who: String = "same-unit" }
  object Case { def who: String = Shared.who }
}

// (1) a class's own member, and (1) a local definition, both beat a wildcard
// import.
package p7 {
  import lib.Wild._
  object Case {
    val member: String = "member"
    def who: String = member
  }
  object Local {
    def who: String = {
      import lib.Wild._
      val local: String = "local"
      local
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(p1.Case.who)
    println(p2.Case.who)
    println(p3.Case.who)
    println(p4.Case.who)
    println(p5.Case.who)
    println(p6.Case.who)
    println(p7.Case.who)
    println(p7.Local.who)
  }
}
