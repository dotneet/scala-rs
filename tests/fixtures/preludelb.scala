// `[B >: A]` on the members `prelude_seq` / `prelude_immutcoll2` had written
// with no type parameter at all: `contains` / `indexOf` / `reduce` /
// `reduceLeft` / `reduceRight` / `toArray` on `List`, and `+` / `updated` on
// `Map`. `agent/lowerbound` fixed the `sorted` family and named these; they
// close through the same `prelude_lowbound` mechanism.
//
// The half that has to be *executed* is the erasure. `sum` moving from `A` to
// `B` was already checked; `reduce` and `reduceLeft` take a **function** of
// the widened type, so the question is sharper for them. Every call below is
// run under `-Xverify:all` and its output compared against real scalac
// 2.13.16's for the same source, at a primitive element type (where a wrong
// erasure boxes or unboxes in the wrong place), at a widened element type,
// and at an element type unrelated to the receiver's.

class Animal(val name: String)
class Dog(name: String) extends Animal(name)
class Cat(name: String) extends Animal(name)

object Main {
  val ints: List[Int] = List(1, 2, 3)
  val rex = new Dog("rex")
  val ace = new Dog("ace")
  val tom = new Cat("tom")
  val dogs: List[Dog] = List(rex, ace)

  def names(xs: List[Animal]): String = xs.map(_.name).mkString(",")

  def main(args: Array[String]): Unit = {
    // ---- reduce / reduceLeft / reduceRight at a primitive element type.
    // `B` is solved at the lower bound `Int`, and the result is unboxed.
    println(ints.reduce(_ + _))
    println(ints.reduceLeft(_ + _))
    println(ints.reduceRight(_ - _))
    val total: Int = ints.reduce(_ * _)
    println(total)

    // The same three widened by the function's own parameter types. nsc's
    // `reduceLeft` keeps its *right* operand at the element type and
    // `reduceRight` its left, so these three functions are not
    // interchangeable and a symmetric declaration would take all of them.
    val a1: Animal = dogs.reduce((x: Animal, y: Animal) => x)
    println(a1.name)
    val a2: Animal = dogs.reduceLeft((acc: Animal, d: Dog) => d)
    println(a2.name)
    val a3: Animal = dogs.reduceRight((d: Dog, acc: Animal) => acc)
    println(a3.name)

    // Widened past every element type by an explicit type argument: the
    // result is `Any` and nothing is unboxed at all.
    val any: Any = ints.reduce[Any]((x, y) => x)
    println(any)

    // ---- contains / indexOf. nsc puts the bound on the element searched
    // for, so an argument of an unrelated type is legal and answers false.
    println(dogs.contains(tom))
    println(dogs.contains(rex))
    println(dogs.indexOf(ace))
    println(dogs.indexOf(tom))
    println(dogs.indexOf(ace, 1))
    println(ints.contains(2))
    println(ints.contains("2"))
    println(ints.indexOf(3))
    println(ints.indexOf("3"))

    // ---- toArray. `Array[B]`'s erasure is `Object`, exactly as `Array[A]`'s
    // was; the widened call is a real `Array[Animal]` at run time.
    val widened: Array[Animal] = dogs.toArray(scala.reflect.ClassTag(classOf[Animal]))
    println(widened.length + ":" + widened(0).name)
    val same: Array[Dog] = dogs.toArray
    println(same.length + ":" + same(1).name)
    val prims: Array[Int] = ints.toArray
    println(prims.length + ":" + prims(2))

    // ---- Map.+ and Map.updated widen the value type.
    val md: Map[String, Dog] = Map("a" -> rex)
    val plus: Map[String, Animal] = md + (("b", tom))
    println(plus("a").name + "," + plus("b").name)
    val upd: Map[String, Animal] = md.updated("b", tom)
    println(upd("b").name)
    // Unwidened, both still answer at the element type.
    val kept: Map[String, Dog] = md.updated("c", ace)
    println(kept("c").name)

    // ---- and the cats shape this started from.
    val w = new Wrapped[String, Dog](md)
    println(w.updated[Animal]("z", tom)("z").name)
    println(w.updated("z", ace)("z").name)
    println(names(List(rex, ace, tom).reduce((x: Animal, y: Animal) => x) :: Nil))
  }
}

// cats' `kernel/compat/WrappedMutableMapBase`, the one cats error this slice
// closes: a wrapper's own `[V2 >: V]` handed straight to `Map`'s `+`, which
// had no type parameter at all and rejected the pair as
// `found: Tuple2[K, V2] required: Tuple2[K, V]`.
class Wrapped[K, V](m: Map[K, V]) {
  def updated[V2 >: V](key: K, value: V2): Map[K, V2] = m + ((key, value))
  def removed(key: K): Map[K, V] = m - key
}
