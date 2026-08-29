// Third `type mismatch` slice on slick: an abstract type member that an alias
// overrides, a type parameter no argument can pin, `this.type` seen from the
// receiver, a block that is not an argument, and protected access from a
// nested anonymous subclass.
//
// Every definition here is accepted by scalac 2.13.16; the expected output is
// what nsc prints for the same program.

// -- an alias overrides the abstract member it inherits twice ---------------
trait Node3 {
  type Self >: this.type <: Node3
  def self: Self
}
abstract class Base3[T](val label: T) extends Node3 {
  type Self = Base3[T]
}
trait Extra3 extends Node3

object Nodes3 {
  // `Extra3` reaches `Node3`'s deferred `type Self` again; the alias declared
  // by `Base3` is the one that wins, and its `T` is *this* `T`.
  def make[T](t: T): Base3[T] = new Base3[T](t) with Extra3 {
    def self: Self = this
    def again: Self = make(label)
  }
}

// -- a type parameter that occurs in no parameter type ----------------------
trait NoStream3
trait Effect3
class Act3[+R, +S <: NoStream3, -E <: Effect3](val r: R)
object Act3 {
  // `S` and `E` are named by nothing an argument could pin, so the call
  // instantiates them to a bound (`Nothing` covariantly, the upper bound
  // contravariantly) rather than leaving `Act3[Int, S, E]` behind.
  def act[R, S <: NoStream3, E <: Effect3](f: Int => R): Act3[R, S, E] = new Act3(f(0))
}

// -- `this.type` is the receiver, arguments and all -------------------------
class Builder3[T] {
  private var items: List[T] = Nil
  def add(v: T): this.type = {
    items = v :: items
    this
  }
  def result: List[T] = items.reverse
}
object Builder3 {
  def newBuilder[T](capacity: Int = 16): Builder3[T] = new Builder3[T]
}

// -- a block after `new C with T { … }` starts a new statement --------------
object Fn3 {
  def mk(n: String): Int => Base3[String] = {
    def build(i: Int): Base3[String] = new Base3[String](n + i) with Extra3 {
      def self: Self = this
    }
    { (i: Int) => build(i) }
  }
}

// -- protected access from an anonymous subclass written inside the class ---
class DDL3(val stmts: List[String]) { self =>
  protected def phase: List[String] = stmts
  def show: List[String] = phase
  // `self` and `other` are `DDL3`s, not instances of this anonymous subclass;
  // the access is legal because the code sits in `DDL3`'s own template.
  def merge(other: DDL3): DDL3 = new DDL3(Nil) {
    override protected def phase: List[String] = self.phase ++ other.phase
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Nodes3.make("n").self.label)
    println(Nodes3.make(4).self.label)

    val a: Act3[Int, Nothing, Effect3] = Act3.act(i => i + 1)
    println(a.r)

    // `newBuilder()` is a `Builder3[?T]`; the `add` applied to it says what
    // `?T` is, and `add` gives the receiver back with its argument.
    val b: List[String] = Builder3.newBuilder().add("x").add("y").result
    println(b)

    println(Fn3.mk("f")(2).label)
    println(new DDL3(List("a")).merge(new DDL3(List("b"))).show)
  }
}
