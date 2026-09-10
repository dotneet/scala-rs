trait Value[A]; class Named extends Value[String]
class Invariant[A](val value:A)
object Main {
 def take[A](p:Invariant[Value[A]]):Unit=()
 take(new Invariant(new Named))
}
