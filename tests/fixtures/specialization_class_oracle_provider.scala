import scala.specialized

/**
 * A small class-owned specialization ABI oracle.  This fixture is compiled
 * only by real scalac in specialization_class_oracle.sh; scala-rs is expected
 * to remain red here until the class specialization phase exists.
 */
class OracleBox[@specialized(Int, Long) A](var value: A) {
  def get: A = value
  def set(v: A): Unit = { value = v }
  def fallback[B](v: B): B = v
}

class OracleIntBox extends OracleBox[Int](1) {
  override def get: Int = value + 10
}
class OracleLongBox extends OracleBox[Long](2L) {
  override def get: Long = value + 20L
}
class OracleStringBox extends OracleBox[String]("s")

trait OracleReadable[@specialized(Int, Long) A] {
  def read: A
}

class OracleReadableInt extends OracleBox[Int](3) with OracleReadable[Int] {
  override def read: Int = get
}
