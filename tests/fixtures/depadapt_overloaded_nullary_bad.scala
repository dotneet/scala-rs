class State[A]
object States {
  def stopped[A <: java.lang.Number]: State[A] = new State[A]
  def stopped[A <: java.lang.Number](callback: () => Unit): State[A] = new State[A]
}
object Main {
  val invalid: State[String] = States.stopped
}
