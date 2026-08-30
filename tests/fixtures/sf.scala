// `Seq[+A] <: PartialFunction[Int, A] <: Int => A`. 2.13's
// `scala.collection.Seq` declaration itself extends `PartialFunction[Int, A]`
// (`javap scala.collection.Seq`), so every `Seq` -- `List`, `Vector`,
// `mutable.ArraySeq`, `mutable.ArrayBuffer`, `IndexedSeq`, `WrappedString`
// (via the `wrapString` implicit) -- is usable wherever an `Int => A` (or a
// `PartialFunction[Int, A]`, complete with `isDefinedAt` / `applyOrElse` /
// `lift` / `orElse`) is wanted. `Array` reaches the same place one step
// removed, through `Predef.wrapBooleanArray: Array[Boolean] =>
// mutable.ArraySeq[Boolean]`.
//
// Every definition here is accepted by scalac 2.13.16; the expected output is
// what nsc prints for the same program.

import scala.collection.mutable.ArrayBuffer

object SeqAsFn {
  val xs: List[Int] = List(10, 20, 30)
  // Direct assignment: `List[Int] <: Int => Int` via subtyping, no explicit
  // conversion tree needed.
  val asFn: Int => Int = xs
  // Passed as an ordinary argument where a function is wanted.
  val picked: List[Int] = List(0, 2).map(xs)

  val v: Vector[String] = Vector("a", "b", "c")
  val vAsFn: Int => String = v

  val ab: ArrayBuffer[Int] = ArrayBuffer(7, 8, 9)
  val abAsFn: Int => Int = ab

  // `Dog <: Animal`, and `Seq` is covariant: `List[Dog] <: Int => Animal`.
  class Animal(val name: String)
  class Dog(name: String) extends Animal(name)
  val dogs: List[Dog] = List(new Dog("Rex"), new Dog("Fido"))
  val dogsAsFn: Int => Animal = dogs
}

object PartialFn {
  val xs = List(1, 2, 3)
  val defined: Boolean = xs.isDefinedAt(1)
  val outOfRange: Boolean = xs.isDefinedAt(5)
  val lifted: Int => Option[Int] = xs.lift
  val lifted1: Option[Int] = lifted(1)
  val lifted5: Option[Int] = lifted(5)
  val fallback: PartialFunction[Int, Int] = { case n if n < 0 => -1 }
  val combined: PartialFunction[Int, Int] = xs.orElse(fallback)
  val fromList: Int = combined(0)
  val fromFallback: Int = combined(-7)
}

object StringAsFn {
  // `Predef.wrapString` is an ordinary implicit `String => WrappedString`,
  // and `WrappedString extends ... Seq[Char]`, so this needs no extra wiring
  // beyond the `Seq <: PartialFunction[Int, A]` edge itself.
  val str = "abcd"
  val charAt: Int => Char = str
  val strDefined: Boolean = str.isDefinedAt(1)
}

object ArrayAsFn {
  // `Array[Boolean]` is not itself a `Seq`, but `Predef.wrapBooleanArray`
  // turns it into one; the argument-position case (`filter`, not just plain
  // assignment) exercises the same view.
  val arr: Array[Boolean] = Array(true, false, true)
  val arrAsFn: Int => Boolean = arr
  val kept: List[Int] = List(0, 1, 2).filter(arr)

  val sieve: Array[Boolean] = Array.fill(10)(true)
  sieve(0) = false
  sieve(1) = false
  val notPrime: List[Int] = (0 until 10).toList.filter(i => !sieve(i))
}

object Main {
  def main(args: Array[String]): Unit = {
    println(SeqAsFn.asFn(1))
    println(SeqAsFn.picked)
    println(SeqAsFn.vAsFn(2))
    println(SeqAsFn.abAsFn(0))
    println(SeqAsFn.dogsAsFn(0).name)

    println(PartialFn.defined)
    println(PartialFn.outOfRange)
    println(PartialFn.lifted1)
    println(PartialFn.lifted5)
    println(PartialFn.fromList)
    println(PartialFn.fromFallback)

    println(StringAsFn.charAt(2))
    println(StringAsFn.strDefined)

    println(ArrayAsFn.arrAsFn(1))
    println(ArrayAsFn.kept)
    println(ArrayAsFn.notPrime)
  }
}
