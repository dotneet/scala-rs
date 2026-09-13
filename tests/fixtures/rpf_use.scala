package resultprefix

import resultprefix.Driver.blockingApi._

object Client {
  def main(args: Array[String]): Unit = println(Database.forURL("x"))
}
