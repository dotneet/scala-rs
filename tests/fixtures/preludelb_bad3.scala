// The bound is still a bound, and the middle line is the neighbour that
// separates it from a rule that merely widens everything.
//
//   `a` -- an `op` whose operands are *below* the element type. `B >: A`
//          cannot be solved at `Cat` for a `List[Dog]`, and nsc reports the
//          join it settled on, `(Animal, Animal) => Animal`, not `(Dog, Dog)
//          => Dog`.
//   `b` -- accepted, by nsc and here. `reduceLeft[B >: A](op: (B, A) => B)`
//          is contravariant in both operands, so a function that *widens*
//          the element operand fits with `B := Dog`. If the widening were
//          applied blindly to both sides this would be refused.
//   `c` -- the result is `B`, so it does not narrow back to the element
//          type.
//
// The branch point printed three errors here, all of them mentioning the
// class's own `A`: the signature it was checking against was `(A, A) => A`.
class Animal(val name: String)
class Dog(name: String) extends Animal(name)
class Cat(name: String) extends Animal(name)

object Main {
  val dogs: List[Dog] = Nil
  val cat: Cat = new Cat("tom")
  val a = dogs.reduce((x: Cat, y: Cat) => cat)
  val b = dogs.reduceLeft((acc: Dog, d: Animal) => acc)
  val c: Dog = dogs.reduce((x: Animal, y: Animal) => x)
}
