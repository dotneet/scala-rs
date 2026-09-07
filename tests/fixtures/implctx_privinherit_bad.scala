// The other half of the rule. Dropping a candidate that nsc *does* reach
// turns a correct `ambiguous implicit` into a silently wrong choice, so each
// arrangement here has to stay rejected. Real scalac 2.13.16 rejects all
// three of these lines.
class S2(val tag: String)

object Sink2 {
  def use(implicit s: S2): String = s.tag
}

object Other {
  implicit def c: S2 = new S2("c")
}

object Two {
  implicit def a: S2 = new S2("a")
  implicit def b: S2 = new S2("b")
  // Two candidates declared side by side: nothing about nesting or access
  // separates them.
  def sameLevel: String = Sink2.use
}

trait OwnPrivateVsImport {
  import Other.c
  private implicit def mine: S2 = new S2("mine")
  // A class's *own* private member is accessible inside it, so it really is
  // a candidate here and really does compete with the import. nsc does not
  // prefer the nearer one.
  def clash: String = Sink2.use
}

trait PublicBase {
  implicit def inherited: S2 = new S2("inherited")
}

trait InheritedVsImport extends PublicBase {
  import Other.c
  // A *non-private* member is inherited, so this stays ambiguous too: the
  // rule is about access, not about where the candidate was written.
  def clash2: String = Sink2.use
}
