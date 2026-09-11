// Access to private members across companion boundaries, from inner
// classes and closures, private[this], and qualified private -- each needs
// an accessor or direct field access that must pick the right member.
object Main {
  class Account(private var balance: Int) {
    private[this] var log = List.empty[String]
    def deposit(n: Int): this.type = { balance += n; log ::= s"+$n"; this }
    def transferTo(o: Account, n: Int): Unit = { balance -= n; o.balance += n }
    def history: List[String] = log.reverse
    class Statement { def line = s"balance=$balance entries=${log.size}" }
    def adder: Int => Unit = n => balance += n
  }
  object Account { def peek(a: Account): Int = a.balance; def reset(a: Account): Unit = a.balance = 0 }
  class Secretive { private def hidden(x: Int) = x * 7; private val v = 3; def viaLambda = List(1, 2).map(hidden); def viaInner = new { def get = v + hidden(1) }.get }
  object Secretive { def poke(s: Secretive) = s.hidden(s.v) }
  class Outer { private val secret = "s3cr3t"; class Inner { def reveal = secret.reverse }; object Nested { def reveal = secret.length } }
  package_private.Holder.check()
  object package_private { object Holder { private[Main] val x = 5; def check(): Unit = () } }
  def main(args: Array[String]): Unit = {
    val a = new Account(10); val b = new Account(0)
    a.deposit(5).deposit(1)
    a.transferTo(b, 6)
    println(Account.peek(a) + " " + Account.peek(b) + " " + a.history)
    println(new a.Statement().line)
    a.adder(100)
    println(Account.peek(a))
    Account.reset(a); println(Account.peek(a))
    val s = new Secretive
    println(s.viaLambda + " " + s.viaInner + " " + Secretive.poke(s))
    val o = new Outer
    println(new o.Inner().reveal + " " + o.Nested.reveal)
    println(package_private.Holder.x)
  }
}
