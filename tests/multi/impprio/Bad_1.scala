// The other half of SLS 2: two bindings of the *same* precedence in the same
// scope do not resolve at all -- the reference is ambiguous, and real scalac
// 2.13.16 rejects every arrangement below. Picking one of them is a wrong
// program, and before this fixture existed scala-rs picked whichever symbol
// happened to have the lower id.
//
// The negative half is the load-bearing half here: without it, "a wildcard
// import ranks below an explicit one" could drift into "prefer whichever
// candidate we like", which is not a ranking.
package bad {
  object A {
    val x: String = "a"
    def f(i: Int): String = "a"
  }
  object B {
    val x: String = "b"
    def f(s: String): String = "b"
  }
}

// Two wildcard imports offering the same name.
package n1 {
  import bad.A._
  import bad.B._
  object Case { def who: String = x }
}

// Two explicit imports of the same name.
package n2 {
  import bad.A.x
  import bad.B.x
  object Case { def who: String = x }
}

// Two wildcard imports whose `f`s would form a legal overload set if they
// came from one import. They do not, so they are ambiguous instead.
package n3 {
  import bad.A._
  import bad.B._
  object Case { def who: String = f(1) }
}
