// `import scala.{specialized => sp}` renamed the annotation past the check
// that rejects it by name, so `@sp` was silently accepted while
// `@specialized` was diagnosed. Both are diagnosed now, and both are ignored
// under `-no-specialization`.
import scala.{specialized => sp}

object Main {
  def id[@sp(Int) T](t: T): T = t
  def main(args: Array[String]): Unit = println(id(1))
}
