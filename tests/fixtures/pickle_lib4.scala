// Operator members, companion-object members, and members that need an
// implicit the library itself provides.
//
// nsc keeps operator names encoded all the way through: `SetOps` pickles `&`
// as `$amp` and the classfile declares `$amp`, so the encoded name is what the
// lookup and the descriptor search use while the symbol keeps the source name.
//
// `Iterator.from` lives on the companion object, not the trait, so it is
// installed on the companion's module class; codegen loads `Iterator$.MODULE$`
// because the method's owner is a module class.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    println(xs :+ 4)
    println(0 +: xs)
    println(xs ++ List(4, 5))
    println(xs ++: List(9))
    println(Set(1, 2) & Set(2, 3))
    println(Set(1, 2) | Set(3))
    println(Set(1, 2, 3) &~ Set(2))
    println(Set(1, 2) ++ Set(3))
    println(Map("a" -> 1) + ("b" -> 2))
    println(Map("a" -> 1, "b" -> 2) - "a")

    println(Iterator.from(1).take(3).toList)
    println(Iterator.continually(7).take(2).toList)
    println(Iterator.single(5).toList)
    println(List.fill(3)(0))
    println(List.tabulate(3)(i => i * i))
    println(Vector.fill(2)("x"))
    println(Vector.tabulate(3)(i => i + 1))
    println(Set.empty[Int])

    println(xs.sum)
    println(xs.product)
    println(List(1.5, 2.5).sum)
  }
}
