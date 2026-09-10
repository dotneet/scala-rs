object ValueDefaults {
 final class Slot[A](val enabled: Boolean = true) extends AnyVal {
  def describe: String = if (enabled) "on" else "off"
 }
 def default[A]: Slot[A] = new Slot[A]
}
