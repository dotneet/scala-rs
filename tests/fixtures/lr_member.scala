// The *member* `lazy val` path (`bitmap$0` + accessor) has to keep working
// next to the local one: a class field, a trait field mixed into two classes,
// an object field, and a member and a local `lazy val` in the same template.
trait Named {
  lazy val label: String = { println("trait-label"); "N" }
  def show: String = label + label
}

class Box(n: Int) extends Named {
  lazy val doubled: Int = { println("box-doubled"); n * 2 }
  def use(): Int = {
    lazy val local: Int = { println("box-local"); doubled + 1 }
    if (n > 0) local + local else 0
  }
}

object Reg extends Named {
  lazy val tag: String = { println("reg-tag"); "R" }
}

object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box(3)
    println(b.doubled)
    println(b.doubled)
    println(b.use())
    println(b.show)
    println(Reg.tag + Reg.show)
    println(new Box(0).use())
  }
}
