package app.model

case class User(id: Int, name: String, tags: List[String])

object User {
  def anonymous(id: Int): User = User(id, "anon", Nil)
}

trait Repo[A] {
  def all: List[A]
  def find(p: A => Boolean): Option[A] = all.find(p)
}
