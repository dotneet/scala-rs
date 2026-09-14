trait Generic[@specialized(Boolean) A] {
  def render(a: A): String
}

trait BooleanImpl extends Generic[Boolean] {
  override def render(a: Boolean): String = if (a) "yes" else "no"
}

class BooleanValue extends BooleanImpl

object Main {
  def main(args: Array[String]): Unit = {
    val generic: Generic[Boolean] = new BooleanValue
    println(generic.render(true))
  }
}
