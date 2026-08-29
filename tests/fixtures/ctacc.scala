// A constructor parameter is a public accessor, and that accessor is what
// implements an abstract member of a parent. A `case class` makes its first
// parameter list `val`s without the keyword, so it has to do so too --
// otherwise the class type-checks and then dies with an AbstractMethodError.

trait Rep[T] {
  def value: T
}

// Both sides erase to `value()Object`: no bridge, just the accessor.
case class ConstRep[T](value: T) extends Rep[T]

trait Named {
  def n: Int
}

// A primitive accessor: `n()I` on both sides.
case class NumRep(n: Int) extends Named

trait Boxed {
  def unwrap: Any
}

// The parent declares `unwrap()Object` and the child stores an `int`, so the
// accessor needs the bridge nsc emits.
case class IntBox(unwrap: Int) extends Boxed

trait Labelled {
  def label: Any
}

// Narrowing a reference result: `label()String` plus a `label()Object` bridge.
case class StringBox(label: String) extends Labelled

trait HasName {
  def name: String
}

// The same shape written out: an explicit `val` parameter.
class Person(val name: String, val age: Int) extends HasName

trait Counter {
  def c: Int
  def c_=(v: Int): Unit
}

// A `var` parameter implements both the getter and the setter.
class Cell(var c: Int) extends Counter

// Only the *first* parameter list becomes accessors; `extra` is private state.
case class Multi(a: Int, b: String)(extra: Long) {
  def total: Long = a + extra
}

object Main {
  def show(r: Rep[Int]): Int = r.value
  def named(x: Named): Int = x.n
  def boxed(b: Boxed): Any = b.unwrap
  def labelled(l: Labelled): Any = l.label
  def nameOf(h: HasName): String = h.name

  def bump(c: Counter): Int = {
    c.c = c.c + 1
    c.c
  }

  def main(args: Array[String]): Unit = {
    println(show(ConstRep(42)))
    println(ConstRep("hi").value)
    println(named(NumRep(7)))
    println(boxed(IntBox(5)))
    println(labelled(StringBox("tag")))
    println(nameOf(new Person("bob", 3)))
    println(new Person("bob", 3).age)
    println(bump(new Cell(10)))
    val m = Multi(1, "x")(41L)
    println(m.a)
    println(m.b)
    println(m.total)
  }
}
