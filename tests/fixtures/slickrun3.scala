// Regression fixture for `agent/slickrun3`: the defects that stood between
// `tests/slick_run.sh`'s `p12_mapped` and slick's own query compiler.
//
// Every case here was found by running slick that scala-rs compiled, and each
// one is checked against real scalac 2.13.16 (see
// `crates/cli/tests/slickrun3.rs`, which also pins the emitted descriptors).

// --- 1. An abstract type member with a *compound* upper bound erases through
// nsc's `intersectionDominator`, and a class that narrows such a parameter
// owes the wide bridge. slick: `MappedColumnTypeFactory.base(…,
// BaseColumnType[U]): BaseColumnType[T]`, implemented by `MappedJdbcType` at
// `JdbcType`.
trait TypedType[T] { def name: String }
trait BaseTypedType[T] extends TypedType[T]

trait TypesComponent {
  type ColumnType[T] <: TypedType[T]
  type BaseColumnType[T] <: ColumnType[T] with BaseTypedType[T]

  trait ColumnTypeFactory {
    def base[T](tag: String, u: BaseColumnType[T]): BaseColumnType[T]
    def assertNonNull[A](t: BaseColumnType[A]): Unit =
      if (t == null) throw new NullPointerException("null column type")
  }

  val factory: ColumnTypeFactory

  // Reached through the interface, so the call uses the wide descriptor the
  // bridge has to answer.
  def viaFactory[T](tag: String, u: BaseColumnType[T]): String =
    factory.base(tag, u).name
}

trait JdbcType[T] extends BaseTypedType[T]

object JdbcComponent extends TypesComponent {
  type ColumnType[T] = JdbcType[T]
  type BaseColumnType[T] = JdbcType[T]

  object MappedJdbcType extends ColumnTypeFactory {
    // Narrows both the parameter and the result; nsc puts a
    // `(String, TypedType)TypedType` bridge on this class.
    def base[T](tag: String, u: JdbcType[T]): JdbcType[T] = {
      assertNonNull(u)
      new JdbcType[T] { def name = tag + "(" + u.name + ")" }
    }
  }
  val factory: ColumnTypeFactory = MappedJdbcType
  val intType: BaseColumnType[Int] = new JdbcType[Int] { def name = "int" }
}

// --- 2. `intersectionDominator` really does drop the other half of the bound.
// scala-reflect's `type TermName >: Null <: TermNameApi with Name` erases to
// the *interface* `TermNameApi`, which is not the abstract *class* `NameApi`
// that only `Name` brings in, so passing one where the other is expected needs
// a cast at the call site.
abstract class NameApi { def text: String }
trait TermNameApi

trait Names {
  type Name >: Null <: NameApi
  type TermName >: Null <: TermNameApi with Name
  def mkTerm(s: String): TermName
  def render(n: NameApi): String = "<" + n.text + ">"
}

object Universe extends Names {
  class NameImpl(val text: String) extends NameApi with TermNameApi
  type Name = NameImpl
  type TermName = NameImpl
  def mkTerm(s: String): TermName = new NameImpl(s)
}

// --- 3. A local's type is written in its own vocabulary: `map.infer(…)` is
// `map.Self`, not this class's `Self`. slick's `ResultSetMapping
// .withInferredType` destructured a tuple of one and `checkcast`ed a plain
// `Node` to a `ResultSetMapping`.
trait Node {
  type Self >: this.type <: Node
  def infer(depth: Int): Self
  def label: String
}
class Leaf extends Node {
  type Self = Leaf
  def infer(depth: Int): Self = this
  def label = "leaf"
}
class Mapping(val map: Node) extends Node {
  type Self = Mapping
  def infer(depth: Int): Self = this
  def label = "mapping"
  def inferred: String = {
    val (m2, d) = (map.infer(1), 7)
    m2.label + d
  }
}

// --- 4. A block in statement position discards its last expression, so a
// branching last expression is generated in statement mode too. slick's
// `QueryInterpolator.appendString` otherwise left one arm of an inner match
// with a value on the stack ("Inconsistent stackmap frames").
class Buf {
  var n = 0
  def +=(c: Char): Buf = { n += 1; this }
  def nl(): Unit = { n += 100 }
}

// --- 5. A `withFilter` whose result is *not* the receiver keeps its own type.
// slick's `ConstArray.withFilter(p): ConstArrayOp[T]`.
trait ArrayOp[T] { def foreach[R](f: T => R): Unit }
final class ConstArr[T](val xs: List[T]) {
  def foreach[R](f: T => R): Unit = xs.foreach(f)
  def withFilter(p: T => Boolean): ArrayOp[T] = new ArrayOp[T] {
    def foreach[R](f: T => R): Unit = xs.filter(p).foreach(f)
  }
}

// --- 6. `super.m` resolves to the first *concrete* `m`, never to a mixin that
// only re-declares it. slick's `BasicStreamingQueryActionExtensionMethodsImpl`
// narrows `result` covariantly and leaves it abstract, and the `$class` holder
// the call named does not exist at all.
trait Res { def show: String }
class ResA extends Res { def show = "A" }
trait Actions { def result: Res }
trait StreamingActions extends Actions { def result: ResA }
class Query0 extends Actions { def result: Res = new ResA }
class Query1 extends Query0 { override def result: ResA = new ResA }
class Query2 extends Query1 with StreamingActions {
  override def result: ResA = super.result
}

object Main {
  def append(b: Buf, str: String, skip: Boolean): Unit = {
    def appendString(s: String): Unit = {
      val len = s.length
      var pos = 0
      while (pos < len) {
        s.charAt(pos) match {
          case '\\' =>
            pos += 1
            if (pos < len) {
              s.charAt(pos) match {
                case c2 @ ('(' | ')') => if (!skip) b += c2
                case '{' =>
                  if (!skip) {
                    b += '('
                    b.nl()
                  }
                case 'n' => b.nl()
                case c2  => throw new IllegalArgumentException("bad " + c2)
              }
            }
          case c => b += c
        }
        pos += 1
      }
    }
    appendString(str)
  }

  def main(args: Array[String]): Unit = {
    println("base: " + JdbcComponent.viaFactory("mapped", JdbcComponent.intType))
    println("direct: " + JdbcComponent.MappedJdbcType.base[Int]("direct", JdbcComponent.intType).name)

    println("name: " + Universe.render(Universe.mkTerm("xs")))

    println("inferred: " + new Mapping(new Leaf).inferred)

    val b = new Buf
    append(b, "ab\\(cd\\{e\\nf", false)
    append(b, "\\(\\)", true)
    println("buf: " + b.n)

    val c = new ConstArr(List((1, "a"), (2, "b"), (3, "c")))
    for ((n, s: String) <- c) print(n.toString + s + " ")
    for (p <- c if p._1 > 1) print(p._2 + " ")
    println()

    println("super: " + new Query2().result.show)
  }
}
