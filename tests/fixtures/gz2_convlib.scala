// The half that comes *second* on the command line, so its signatures are not
// known while `gz2_convuse.scala` is typed. See that file.
package gz2conv

object Gz2Conv {
  implicit class Rich(private val s: String) extends AnyVal {
    def pick[T](tag: String, params: Any*)(f: Int => T): Seq[T] = Seq(f(s.length), f(tag.length))
  }
}
