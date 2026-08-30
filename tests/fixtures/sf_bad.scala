// `Seq <: PartialFunction[Int, A]` must not turn into "any Seq is any
// function": the domain is fixed at `Int` and the codomain is still
// checked with its own variance. Every definition below is rejected by
// scalac 2.13.16 too (the wording differs).

object Dom {
  val xs = List(1, 2, 3)
  // nsc: type mismatch; found: List[Int]; required: String => Int
  val bad: String => Int = xs
}

object Var {
  class Animal
  class Dog extends Animal
  val animals: List[Animal] = List(new Animal, new Dog)
  // `List` is covariant, but `Animal` is not a `Dog`: nsc: type mismatch;
  // found: List[Var.Animal]; required: Int => Var.Dog
  val bad: Int => Dog = animals
}
