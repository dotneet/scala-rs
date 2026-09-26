// cats' newtype encoding (`NonEmptyMap`, `NonEmptySet`): an abstract type
// member no class ever defines, reached through a package-object alias, and
// the ops that come with it through an implicit conversion in the object that
// owns the member. A factory's result is `NEMapImpl.Type[K, A]`, the
// receiver's path; read off the declaration instead it was the bare
// `Newtype2#Type[K, A]`, whose implicit scope has no `ops`, and cats'
// `NonEmptySet.groupBy` stopped compiling (`value lookup is not a member`).
package object nt2 {
  type NESet[+A] = NESetImpl.Type[A]
  val NESet = NESetImpl
  type NEMap[K, +A] = NEMapImpl.Type[K, A]
  val NEMap = NEMapImpl
}

package nt2 {
  trait Newtype {
    type Base
    trait Tag extends Any
    type Type[+A] <: Base with Tag
  }
  trait Newtype2 {
    type Base
    trait Tag extends Any
    type Type[A, +B] <: Base with Tag
  }
  object NESetImpl extends Newtype {
    def one[A](a: A): NESet[A] = List(a).asInstanceOf[NESet[A]]
    implicit def ops[A](value: Type[A]): NESetOps[A] = new NESetOps(value)
  }
  class NESetOps[A](val value: NESet[A]) {
    def toList: List[A] = value.asInstanceOf[List[A]]
    def add(a: A): NESet[A] = (a :: toList).asInstanceOf[NESet[A]]
    def reduceLeftTo[B](f: A => B)(g: (B, A) => B): B = toList.tail.foldLeft(f(toList.head))(g)
  }
  object NEMapImpl extends Newtype2 {
    def one[K, A](k: K, a: A): NEMap[K, A] = Map(k -> a).asInstanceOf[NEMap[K, A]]
    implicit def ops[K, A](value: Type[K, A]): NEMapOps[K, A] = new NEMapOps(value)
  }
  class NEMapOps[K, A](val value: NEMap[K, A]) {
    def toMap: Map[K, A] = value.asInstanceOf[Map[K, A]]
    def lookup(k: K): Option[A] = toMap.get(k)
    def add(ka: (K, A)): NEMap[K, A] = (toMap + ka).asInstanceOf[NEMap[K, A]]
  }
  object Demo {
    def groupBy[A, B](s: NESet[A])(f: A => B): NEMap[B, NESet[A]] =
      s.reduceLeftTo(a => NEMap.one(f(a), NESet.one(a))) { (acc, a) =>
        val key = f(a)
        val result = acc.lookup(key) match {
          case Some(nes) => nes.add(a)
          case _         => NESet.one(a)
        }
        acc.add((key, result))
      }
    def main(args: Array[String]): Unit = {
      val s = NESet.one(1).add(2).add(3).add(4)
      val m = NEMap.one("k", 1)
      println(List(m.lookup("k"), m.add("j" -> 2).toMap.size))
      println(groupBy(s)(_ % 2).toMap.toList.sortBy(_._1).map { case (k, v) => k -> v.toList.sorted })
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = nt2.Demo.main(args)
}
