class State[A](val name: String)
object States {
  def stopped[A]: State[A] = new State[A]("stopped")
  def stopped[A](callback: () => Unit): State[A] = {
    callback()
    new State[A]("callback")
  }
}
object Main {
  import States.stopped
  def signals: PartialFunction[Int, State[String]] = {
    case 1 => stopped
    case _ => States.stopped
  }
  def main(args: Array[String]): Unit = {
    println(signals(1).name)
    println(signals(2).name)
    println(States.stopped[String](() => println("run")).name)
  }
}
