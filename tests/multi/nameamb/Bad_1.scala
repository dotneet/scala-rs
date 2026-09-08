// The arrangements real scalac 2.13.16 *rejects*: a definition and an
// `import` clause nested more deeply than it. Precedence would let the
// definition win and nesting would let the import win, so nsc reports the
// reference rather than picking -- `Contexts.lookupSymbol` consults an import
// only while `imp1.depth > symbolDepth`, and then `defSym` and `impSym`
// together are `ambiguousDefnAndImport`.
//
// Every one of these *compiles* if the rule is missing, printing the imported
// answer where scalac refuses to choose. That is what `neg/name-lookup-stable`
// in the scala/scala corpus is, and it is why the negative half is the
// load-bearing one.
package nameambbad

object Imp {
  def who: String = "imported"
  object Tag
}

trait Base {
  def who: String = "inherited"
}

// (a) A member of the class, and a wildcard import inside one of its methods.
class Member {
  def who: String = "defined"
  def pick: String = {
    import Imp._
    who
  }
}

// (b) An *inherited* member, and an explicit import inside a method. nsc
//     names the declaring owner, `trait Base`, not the class doing the
//     inheriting.
class Inherits extends Base {
  def pick: String = {
    import Imp.who
    who
  }
}

// (c) A local definition in an outer block, and a wildcard import in an inner
//     one. The owner nsc names here is the enclosing method.
object Local {
  def pick: String = {
    def who: String = "local"
    val r = {
      import Imp._
      who
    }
    r
  }
}

// (d) A stable-id *pattern* is a reference too, and the corpus test's first
//     error is one of these.
class Pattern {
  def Tag: Any = ""
  def pick(a: Any): Boolean = {
    import Imp._
    a match {
      case Tag => true
      case _   => false
    }
  }
}
