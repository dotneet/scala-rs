// agent/dbio: the roots behind slick's `JdbcActionComponent.scala` / `DBIOAction.scala`.
//
//  1. Named arguments in a parent constructor call
//     (`extends SimpleJdbcProfileAction[R](_name = …, statements = …)`).
//  2. Referring to a `private[this]` member unqualified from inside an anonymous
//     class that extends the enclosing class *at different type arguments*
//     (`SynchronousDatabaseAction.superZip` / `superAsTry`).
//  3. Solving `[B >: A]` when the lower bound mentions the caller's own method
//     type parameter (the receiver `PositionedResultIterator[T]` of
//     `Either.getOrElse(throw …)`). Here the same shape goes through `Option`,
//     which exists in non-jar mode too.
//  4. A typed pattern `case a: T[_, …]` keeps the type arguments the scrutinee
//     already states (nsc's `inferTypedPattern`).
//
// So that it also passes under `--no-scala-library`, this is written with
// hand-rolled classes and `Array` only, not `Vector` / `List`.

class Box[R](val first: R)

// 1. Named arguments to the parent constructor: reordered, in order, and defaulted.
abstract class Act(_name: String, statement: String, repeat: Int = 1) {
  def show: String = {
    var s = _name + "["
    var i = 0
    while (i < repeat) {
      s = s + statement
      i = i + 1
    }
    s + "]"
  }
}

class Reordered(n: Int)
    extends Act(
      statement = if (n > 0) "one" else "all",
      _name = "Reordered"
    )

class InOrder(n: Int)
    extends Act(
      _name = "InOrder",
      statement = "s" + n.toString,
      repeat = 2
    )

// 2. A `private[this]` parent member. The anonymous subclass is `Outer[Box[R]]`,
//    so reading `base` "through this class" would give `Box[Box[R]]`.
//    If `base` were public scalac reports the same mismatch (the inherited one
//    shadows the outer one), so this shape is specific to `private[this]`.
abstract class Outer[R](val r: R) {
  private[this] def base: Box[R] = new Box[R](r)

  def wrap: String = {
    // The parent constructor argument is hoisted into a local (reading the outer
    // `this` from inside an anonymous class's `<init>` hits a known codegen bug
    // unrelated to this slice -- a `getfield` on `uninitializedThis`).
    val seed = new Box[R](r)
    val o = new Outer[Box[R]](seed) {
      val nonFused: Box[R] = base
      override def toString = "wrapped:" + nonFused.first.toString
    }
    o.toString
  }
}

// 4. `case a: T[_, …]` keeps the type arguments the scrutinee already states
//    (nsc's `inferTypedPattern`). Binding `Sync[_, _, _]` bare leaves nothing
//    that can be passed to `superZip`'s `Zip[R2, E2]`.
trait Eff
trait Zip[+R, -E <: Eff] {
  def tag: String
  def zip[R2, E2 <: Eff](a: Zip[R2, E2]): Zip[R2, E with E2] =
    new Zipped[R2, E with E2]("plain(" + tag + "," + a.tag + ")")
}
class Zipped[R, E <: Eff](val tag: String) extends Zip[R, E]
trait Sync[+R, C, -E <: Eff] extends Zip[R, E] {
  private[this] def superZip[R2, E2 <: Eff](a: Zip[R2, E2]): Zip[R2, E with E2] =
    super.zip[R2, E2](a)
  override def zip[R2, E2 <: Eff](a: Zip[R2, E2]): Zip[R2, E with E2] = a match {
    case s: Sync[_, _, _] => new Zipped[R2, E with E2]("fused(" + superZip(s).tag + ")")
    case other            => superZip(other)
  }
}
class SyncAct[R](val tag: String) extends Sync[R, String, Eff]

object Main {
  // 3. The `A` of the lower bound `B >: A` mentions the caller's type parameter.
  //    The argument is `Nothing`, so without the bound `B` solves to `Nothing`.
  def firstOf[T](o: Option[Box[T]]): T =
    o.getOrElse(throw new RuntimeException("empty")).first

  def main(args: Array[String]): Unit = {
    println(new Reordered(1).show)
    println(new Reordered(0).show)
    println(new InOrder(7).show)
    println(new Outer[String]("x") {}.wrap)
    println(firstOf(Some(new Box(41))))
    val sync = new SyncAct[Int]("sync")
    println(sync.zip(new SyncAct[Int]("other")).tag)
    println(sync.zip(new Zipped[Int, Eff]("plainRhs")).tag)
  }
}
