// scalac: inferred type arguments [String] do not conform to method f's
// type parameter bounds [T <: AnyVal].
object Main {
  def f[T <: AnyVal](t: T): T = t
  def main(args: Array[String]): Unit = println(f("s"))
}
