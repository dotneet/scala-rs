// scalac 2.13.16: "redefinition of equals method. See SIP-15, criterion 5.
// is not allowed in value class" (and the same for hashCode).
class V(val x: Int) extends AnyVal { override def equals(o: Any) = true }
class W(val x: Int) extends AnyVal { override def hashCode = 1 }
object Main { def main(a: Array[String]): Unit = println(new V(1) == new V(2)) }
