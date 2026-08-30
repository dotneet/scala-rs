object Main {
  class Account(private var balance: Int) {
    def deposit(n: Int): Unit = { require(n > 0); balance += n }
    def withdraw(n: Int): Boolean = if (n <= balance) { balance -= n; true } else false
    def get: Int = balance
  }
  object Bank {
    private val accts = scala.collection.mutable.Map[String, Account]()
    def open(id: String, init: Int): Account = { val a = new Account(init); accts(id) = a; a }
    def total: Int = accts.values.map(_.get).sum
    def find(id: String): Option[Account] = accts.get(id)
  }
  def main(a: Array[String]): Unit = {
    Bank.open("x", 100); Bank.open("y", 50)
    Bank.find("x").foreach(_.deposit(25))
    println(Bank.total)
    println(Bank.find("x").map(_.withdraw(200)))
    println(Bank.find("z").isEmpty)
    println(Bank.find("y").fold(0)(_.get))
  }
}
