// `-Xsource:3`: `A & B` is the Scala 3 spelling of the 2.13 compound type
// `A with B`. Requires the flag; plain 2.13 reports `not found: type &`.
trait Named {
  def name: String
}
trait Aged {
  def age: Int
}
class Person(n: String, a: Int) extends Named with Aged {
  def name: String = n
  def age: Int = a
}

object Main {
  type Both = Named & Aged

  // A compound upper bound, both spellings mixed in one type.
  def describe[A <: Named & Aged with Both](a: A): String = "bounded"

  def show(x: Named & Aged): String = x.name

  def age(x: Both): Int = x.age

  def main(args: Array[String]): Unit = {
    val p = new Person("ada", 36)
    println(describe(p))
    println(show(p))
    println(age(p))
    val b: Both = p
    println(b.age)
    println(new Both3(p).twice)
  }

  class Both3(val v: Named & Aged) {
    def twice: Int = v.age * 2
  }
}
