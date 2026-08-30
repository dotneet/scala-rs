// No stubbing: reading `Future.apply`'s real signature out of the companion's
// pickle means its *implicit* clause is real too. Without an
// `ExecutionContext` in scope the call is rejected, exactly as scalac rejects
// it -- it does not quietly succeed just because the by-name parameter now
// matches.

import scala.concurrent.Future

object Main {
  def main(args: Array[String]): Unit = {
    println(Future(21).value)
  }
}
