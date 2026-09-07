// SLS 5.2: a `private` member is not inherited, so it is not an implicit
// candidate anywhere but inside its own class. Three routes reached one
// anyway, and each turned a call nsc resolves into `ambiguous implicit`.
//
// Every case prints which implicit actually ran, because picking the other
// one is a wrong answer at run time rather than a compile error. The expected
// output is real scalac 2.13.16's for this same source.
class Session(val tag: String)

object Sink {
  def use(implicit s: Session): String = s.tag
}

object Named {
  implicit def viaImport: Session = new Session("viaImport")
}

trait Owner {
  private implicit def ownPrivate: Session = new Session("ownPrivate")
  // Inside its own class the private implicit is a candidate, and here it is
  // the only one: nothing has imported `viaImport` into this scope.
  def inOwner: String = Sink.use
}

// Route 1 -- inheritance. `Sub` does not inherit `ownPrivate`, so the
// imported candidate stands alone.
trait Sub extends Owner {
  import Named.viaImport
  def inSub: String = Sink.use
}

// Route 2 -- self type. `self: Owner =>` makes Owner's members visible
// unqualified, but it is a conformance obligation, not membership: it does
// not widen access to Owner's private ones.
trait ViaSelf { self: Owner =>
  import Named.viaImport
  def inSelf: String = Sink.use
}

// Route 3 -- wildcard import. `Holder` only *inherits* `ownPrivate`, so
// `import Holder._` cannot name it.
object Holder extends Owner

object ViaWildcard {
  import Holder._
  import Named.viaImport
  def inWildcard: String = Sink.use
}

object Main extends Sub with ViaSelf {
  def main(args: Array[String]): Unit = {
    println(inOwner)
    println(inSub)
    println(inSelf)
    println(ViaWildcard.inWildcard)
  }
}
