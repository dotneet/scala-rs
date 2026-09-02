// SLS 9.2: a `package` clause opens the package it *names*. The nested
// spelling `package p { package q { … } }` therefore opens both `p` and `q`,
// so `p`'s own members are in scope unqualified and a subpackage `p.r`
// shadows a top-level `r`. (The qualified spelling `package p.q` opens only
// `p.q`; that one needs a second file, and lives in
// `crates/cli/tests/proj.rs`.)
package pjpkg {
  object Cfg { val n = 41 }
}

package pjpkg {
  package inner {
    object Deep { val d = 1 }
  }
}

package inner {
  object Deep { val d = 2 }
}

package pjpkg {
  package sub {
    object Use {
      val a = Cfg.n
      val b = inner.Deep.d
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(pjpkg.sub.Use.a)
    println(pjpkg.sub.Use.b)
    // `Main` is in the empty package, so `inner` here is the top-level one.
    println(inner.Deep.d)
  }
}
