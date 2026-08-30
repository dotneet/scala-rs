object Main {
  sealed trait Cmd
  case class Deposit(n: Int) extends Cmd
  case class Withdraw(n: Int) extends Cmd
  def run(bal: Int, cmds: List[Cmd]): Either[String, Int] = cmds match {
    case Nil => Right(bal)
    case Deposit(n) :: t => run(bal + n, t)
    case Withdraw(n) :: t => if (n > bal) Left(s"insufficient: $n > $bal") else run(bal - n, t)
  }
  def main(a: Array[String]): Unit = {
    println(run(100, List(Deposit(50), Withdraw(30))))
    println(run(10, List(Withdraw(30))))
    val ledger = List(Deposit(10), Withdraw(5), Deposit(3))
    println(ledger.foldLeft(0) { case (b, Deposit(n)) => b + n; case (b, Withdraw(n)) => b - n })
    println(ledger.collect { case Deposit(n) if n > 5 => n }.sum)
    println(ledger.span(_.isInstanceOf[Deposit]))
  }
}
