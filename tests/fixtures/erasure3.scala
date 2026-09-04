// Three erasure/bridge gaps that each stopped slick's run harness one line
// further on. Real scalac 2.13.16's own stdout is the expectation.

// ---------------------------------------------------------------------------
// 1. The dominator of a compound type (SLS 3.7, nsc `intersectionDominator`).
//    Not `parents.head`: the first parent that is a class rather than a trait
//    and that no other parent is a subclass of. slick writes
//    `q: Query[T, U, Seq] & TableQuery[T]`, and `TableQuery <: Query`, so the
//    descriptor nsc gives the method -- and that a separately compiled client
//    calls -- takes a `TableQuery`.
class Base { def base = "base" }
class Derived extends Base { def derived = "derived" }
trait Marker { def mark = "mark" }

object Dominator {
  // Erases to `(Derived)String`: `Base` is shadowed by `Derived`.
  def shadowed(x: Base with Derived): String = x.base + "/" + x.derived
  // No parent is a class but `Base`, so it wins over the trait written first.
  def traitFirst(x: Marker with Base): String = x.mark + "/" + x.base
}

// ---------------------------------------------------------------------------
// 2. A bridge for an inherited member whose *parameter* the implementation
//    narrowed. slick's `RelationalActionComponent` declares
//    `createSchemaActionExtensionMethods(_: SchemaDescription)` over an
//    abstract type that `SqlProfile` fixes, so only the narrow descriptor was
//    implemented and a call through the base interface was an
//    `AbstractMethodError`.
class Wide { override def toString = "wide" }
class NarrowArg extends Wide { override def toString = "narrow" }

trait TakesWide {
  type Arg <: Wide
  def take(a: Arg): String
}

trait TakesNarrow extends TakesWide {
  type Arg = NarrowArg
  def take(a: NarrowArg): String = "took " + a
}

object Impl extends TakesNarrow

// ---------------------------------------------------------------------------
// 3. A lambda parameter whose type is still a tuple after erasure took its
//    arity from nowhere: the cast was hard-coded to `Tuple2`, so
//    `.map(_._2)` over a `Box[(A, B, C)]` cast to `Tuple2` and then called
//    `Tuple3._2` on it. The verifier threw the whole method out.
class Box[A](val a: A) {
  def map[B](f: A => B): Box[B] = new Box(f(a))
}

object Box {
  def mk[A](a: A): Box[A] = new Box(a)
}

object Main {
  def main(args: Array[String]): Unit = {
    val d = new Derived with Marker
    println(Dominator.shadowed(d))
    println(Dominator.traitFirst(d))

    // The wide call cannot be written in Scala -- `w.Arg` is abstract -- so it
    // is reached the way a separately compiled caller reaches it, through the
    // interface's own descriptor. The test also checks the bridge with javap.
    println(Impl.take(new NarrowArg))
    val take = classOf[TakesWide].getMethod("take", classOf[Wide])
    println(take.invoke(Impl, new NarrowArg))

    println(Box.mk[(String, Int, Boolean)](("a", 1, true)).map(_._2).a)
    println(Box.mk[(String, Int, Boolean, Double)](("a", 1, true, 2.5)).map(_._4).a)
    println(Box.mk[(String, Int)](("a", 1)).map(_._1).a)
  }
}
