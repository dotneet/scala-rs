// The fourth slice must not turn its relaxations into silence. scalac 2.13.16
// rejects every definition below (the wording differs).

// An alias that now resolves is still checked: `Fixed5[Unit, Eff5]` is not a
// `Fixed5[String, Eff5]`, contravariance in `E` or no.
package hidden5 {
  trait Eff5
  trait Fixed5[+R, -E <: Eff5]
}
package app5 {
  import hidden5.{Eff5, Fixed5}
  trait Comp5 {
    type PA5[+R, -E <: Eff5] = Fixed5[R, E]
    abstract class Simple5 extends PA5[Unit, Eff5]
    // nsc: type mismatch; found: Simple5; required: Comp5.this.PA5[String, Eff5]
    def wrong: PA5[String, Eff5] = new Simple5 {}
  }
}

// Reading the compound type through the left side is not a licence to drop a
// parent: `A5[R]` alone does not conform to `A5[R] with C5[R]`.
trait A5[+R]
trait B5[+R] extends A5[R]
trait C5[+R]
trait P5 {
  type N5[+R] <: A5[R] with C5[R]
}
trait Q5 extends P5 {
  // nsc: overriding type N5 in trait P5; type N5 has incompatible type
  type N5[+R] <: B5[R]
}

// `Map[K, V]` is a `K => V` and nothing wider: the key type still has to fit.
object Fn5 {
  val m: Map[String, Int] = Map("a" -> 1)
  // nsc: type mismatch; found: Map[String,Int]; required: Int => Int
  val bad: Int => Int = m
}

// `type Self >: this.type` admits `this` and nothing else: another value of
// the same class is not the singleton the lower bound names.
trait Nd5 {
  type Self >: this.type <: Nd5
}
object Self5 {
  // nsc: type mismatch; found: b.type (with underlying type Nd5); required: a.Self
  def wrong(a: Nd5, b: Nd5): a.Self = b
}

// `map` keeps the receiver's collection, which is not a licence to narrow:
// a `Seq` does not become an `IndexedSeq` by being mapped.
object Coll5 {
  // nsc: type mismatch; found: Seq[String]; required: IndexedSeq[String]
  val bad: IndexedSeq[String] = Seq(1, 2).map(_.toString)
}
