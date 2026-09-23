// `toArray` had two candidates: `prelude_seq`'s monomorphic
// `(implicit ClassTag[A]): Array[A]`, which a `ClassTag[Animal]` does not
// fit, and the pickled `IterableOnceOps.toArray[B >: A]`, which does but
// whose `B` was never instantiated. The branch point rejected this file too,
// and that is the point: it rejected it with `found: Array[B]`, naming a
// type parameter of a signature the programmer never wrote.
//
// Both compilers refuse it, at different ends of the same call. nsc
// minimises `B` over the expected type -- `Array[Dog]` gives `B := Dog` --
// and then the argument is wrong: `found: ClassTag[Animal] required:
// ClassTag[Dog]`. This compiler now minimises `B` the same way and
// reports the same argument. What is asserted is that message and that no
// uninstantiated `B` reaches it.
class Animal
class Dog extends Animal

object Main {
  val dogs: List[Dog] = Nil
  val ctA: scala.reflect.ClassTag[Animal] = scala.reflect.ClassTag(classOf[Animal])
  val narrow: Array[Dog] = dogs.toArray(ctA)
}
