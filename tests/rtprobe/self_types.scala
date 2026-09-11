// Self types and the cake pattern: members of the self type reached through
// `this`, self-type aliases, and a component graph wired in one object.
object Main {
  trait Logging { def log(s: String): String = "[log] " + s }
  trait Repo { self: Logging => def find(id: Int): String = log("find " + id) }
  class Service extends Repo with Logging
  trait UserRepoComp { val userRepo: UserRepo; class UserRepo { def name(id: Int) = "user" + id } }
  trait UserServiceComp { this: UserRepoComp => val userService: UserService; class UserService { def greet(id: Int) = "hi " + userRepo.name(id) } }
  object Registry extends UserServiceComp with UserRepoComp { val userRepo = new UserRepo; val userService = new UserService }
  trait Node { outer => val label: String; class Child { def parentLabel = outer.label } }
  class N(val label: String) extends Node
  trait Ordered2 { self: { def rank: Int } => def rankTwice: Int = self.rank * 2 }
  class Ranked(val rank: Int) extends Ordered2
  trait Stack[A] { this: scala.collection.mutable.ArrayBuffer[A] => def pushAll(xs: A*): this.type = { this ++= xs; this } }
  def main(args: Array[String]): Unit = {
    println(new Service().find(3))
    println(Registry.userService.greet(7))
    val n = new N("root"); val c = new n.Child
    println(c.parentLabel)
    println(new Ranked(21).rankTwice)
    val st = new scala.collection.mutable.ArrayBuffer[Int] with Stack[Int]
    println(st.pushAll(1, 2, 3).sum)
  }
}
