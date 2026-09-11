// Implicit resolution priority: local over imported, subclass trait over
// low-priority parent, companion scope of the type, default when absent,
// and explicit arguments.
object Main {
  trait Tag[A] { def name: String }
  object Tag {
    implicit val intTag: Tag[Int] = new Tag[Int] { def name = "companion-int" }
    implicit def listTag[A](implicit a: Tag[A]): Tag[List[A]] = new Tag[List[A]] { def name = "list of " + a.name }
  }
  def tag[A](implicit t: Tag[A]): String = t.name

  trait Low { implicit def fallback[A]: Pri[A] = new Pri[A]("low") }
  object Pri extends Low { implicit val forString: Pri[String] = new Pri[String]("high-string") }
  class Pri[A](val who: String)
  def pri[A](implicit p: Pri[A]): String = p.who

  case class Config(v: String)
  def withDefault(x: Int)(implicit c: Config = Config("default")): String = s"$x ${c.v}"

  object Imports { implicit val importedTag: Tag[String] = new Tag[String] { def name = "imported-string" } }

  class Animal; class Dog extends Animal
  trait Namer[-A] { def n: String }
  implicit val animalNamer: Namer[Animal] = new Namer[Animal] { def n = "animal namer" }
  def namer[A](implicit nm: Namer[A]): String = nm.n

  def main(args: Array[String]): Unit = {
    println(tag[Int]); println(tag[List[List[Int]]])
    locally {
      implicit val localInt: Tag[Int] = new Tag[Int] { def name = "local-int" }
      println(tag[Int]); println(tag[List[Int]])
    }
    import Imports._
    println(tag[String])
    println(pri[String]); println(pri[Int])
    println(withDefault(1))
    locally { implicit val cfg: Config = Config("local"); println(withDefault(2)) }
    println(withDefault(3)(Config("explicit")))
    println(namer[Dog])
    println(implicitly[Ordering[Int]].compare(1, 2)); println(implicitly[Numeric[Double]].plus(1.5, 2))
    println(implicitly[Tag[Int]].name)
  }
}
