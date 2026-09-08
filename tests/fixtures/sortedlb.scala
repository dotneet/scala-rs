// `[B >: A]` on a member whose *only* parameter clause is implicit:
// `def sorted[B >: A](implicit ord: Ordering[B]): C`.
//
// The lower bound does not fix `B`. An explicitly supplied argument
// determines it (`xs.sorted(AA.toOrdering)` with `AA >: A` is `B := AA`,
// which is how cats writes six of its collection wrappers), and only when
// the argument comes from implicit search does the bound decide.

// A cats-shaped type class, so the fixture does not depend on cats itself.
trait Order[A] {
  def compare(x: A, y: A): Int
  def toOrdering: Ordering[A] = new Ordering[A] {
    def compare(x: A, y: A): Int = Order.this.compare(x, y)
  }
}

class Animal(val name: String)
class Dog(name: String) extends Animal(name)

object ByName extends Order[Animal] {
  def compare(x: Animal, y: Animal): Int = x.name.compareTo(y.name)
}

/// cats' `NonEmptyList#sorted` / `NonEmptyVector#sorted` / `Chain#sorted`,
/// which are all the same shape: a lower-bounded parameter of the *wrapper*
/// handed straight to the collection's own lower-bounded parameter.
final class NEL[+A](val toList: List[A]) {
  def sorted[AA >: A](implicit AA: Order[AA]): NEL[AA] =
    new NEL(toList.sorted(AA.toOrdering))
  def sortedVia[AA >: A](implicit AA: Order[AA]): NEL[AA] =
    new NEL(toList.toVector.sorted(AA.toOrdering).toList)
  // The same call with the type argument written out. nsc takes it; without
  // it the parameter is pinned at the bound instead.
  def sortedExplicit[AA >: A](implicit AA: Order[AA]): NEL[AA] =
    new NEL(toList.sorted[AA](AA.toOrdering))
}

object Main {
  def names(xs: List[Animal]): String = xs.map(_.name).mkString(",")

  def main(args: Array[String]): Unit = {
    val dogs: List[Dog] = List(new Dog("rex"), new Dog("ace"))

    // The explicit-argument direction: `Ordering[Animal]` where the receiver
    // is a `List[Dog]`.
    println(names(dogs.sorted(ByName.toOrdering)))
    println(names(dogs.toVector.sorted(ByName.toOrdering).toList))
    println(names((dogs: Seq[Dog]).sorted(ByName.toOrdering).toList))
    println(names(dogs.to(LazyList).sorted(ByName.toOrdering).toList))
    println(dogs.max(ByName.toOrdering).name)
    println(dogs.min(ByName.toOrdering).name)

    println(names(new NEL(dogs).sorted(ByName).toList))
    println(names(new NEL(dogs).sortedVia(ByName).toList))
    println(names(new NEL(dogs).sortedExplicit(ByName).toList))

    // The implicit-search direction: nothing is written, so the parameter is
    // instantiated at its lower bound and the witness is found for that.
    println(List(3, 1, 2).sorted)
    println(Vector(3, 1, 2).sorted)
    println(List("b", "a").sorted)
    println(List(3, 1, 2).max)
    println(List(3, 1, 2).min)
    println(List(3, 1, 2).sum)
    println(List(3, 1, 2).product)
    println(List(3, 1, 2).maxOption)

    // The same, with an expected type. This is the path that has no
    // argument *and* no free choice of result, and it is where a parameter
    // left undetermined shows up as "could not find implicit value".
    val ss: List[String] = List("b", "a").filter(_.nonEmpty).sorted
    println(ss)
    val vs: Seq[Int] = Vector(2, 1).sorted
    println(vs)
    val m: Int = List(1, 2).max
    println(m)
    val total: Int = List(1, 2).sum
    println(total)
  }
}
