import tablequerybinary._

object TableQueryBinaryUse {
  val account: Option[Account] = Catalog.accounts.filter(_ => true).firstOption
  val selected: Option[User] = Catalog.users.filter(_ => true).firstOption
  val inserted: Int = Catalog.users.insert(new User)
}
