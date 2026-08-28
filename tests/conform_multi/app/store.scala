package app.store

import app.model.{Repo, User}

class UserStore(users: List[User]) extends Repo[User] {
  def all: List[User] = users
  def byName(n: String): Option[User] = find(_.name == n)
  def names: List[String] = users.map(_.name).sorted
}

object UserStore {
  def of(us: User*): UserStore = new UserStore(us.toList)
}
