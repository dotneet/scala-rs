// The arrangements that must stay *unambiguous*, each printing which
// definition it selected. Real scalac 2.13.16 compiles this file and prints
// `expected.txt`. So did scala-rs before `agent/nameamb`, which is the point
// -- the slice adds a rejection, and this file is what says the rejection did
// not spread.
//
// With one line's exception, and it is why a guard fixture is written to
// *run*: the pre-fix binary printed `private-to-this` for (4). `import Priv._`
// was pulling in a `private[this]` member and binding it over the class's own
// definition, so the file compiled and printed a member SLS 5.2 does not let
// out of `Priv`. That is a second wrong program, found by writing the guard,
// and it is fixed here too (`Typer::private_member_visible_here`).
package nameamb

import Exp.who

object Main {
  // (1) The import is *outside* the definition. Precedence and nesting agree
  //     that the definition wins, and nsc never even consults an import that
  //     is not deeper than the scope the definition was found in.
  class OuterImport {
    def who: String = "defined"
    def pick: String = who
  }

  // (2) An explicit import hides a wildcard one in the same scope: SLS 2
  //     level 2 against level 3. Two levels, one scope, no ambiguity.
  object ExplicitBeatsWildcard {
    def pick: String = {
      import Wild._
      import Exp.who
      who
    }
  }

  // (3) A definition and an import at the *same* level. This is the case the
  //     new rule must not touch: one scope, so precedence alone settles it
  //     and the definition wins.
  class SameLevel {
    import Wild._
    def who: String = "defined"
    def pick: String = who
  }

  // (4) The import is deeper, but what it offers under that name is
  //     `private[this]` and invisible here. One candidate, not two.
  class PrivateCandidate {
    def hidden: String = "defined"
    def pick: String = {
      import Priv._
      hidden
    }
  }

  // (5) nsc's exception. `Elsewhere` is made available by this file's package
  //     clause but defined in another unit (`Sib_1.scala`), which is SLS 2
  //     precedence 4 -- below a wildcard import. The deeper import wins and
  //     nothing is ambiguous.
  object PackageElsewhere {
    def pick: String = {
      import Sib._
      Elsewhere.who
    }
  }

  // (6) A local definition and an import in one block: same scope again.
  object SameBlock {
    def pick: String = {
      import Wild._
      def who: String = "local"
      who
    }
  }

  def main(args: Array[String]): Unit = {
    println(new OuterImport().pick)
    println(ExplicitBeatsWildcard.pick)
    println(new SameLevel().pick)
    println(new PrivateCandidate().pick)
    println(PackageElsewhere.pick)
    println(SameBlock.pick)
    // (7) The file-level `import Exp.who` on its own, with no definition to
    //     compete with.
    println(who)
  }
}
