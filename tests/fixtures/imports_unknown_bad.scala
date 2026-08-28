// `Nope` is not a member of `p1`: scalac reports the selector itself, not
// only the later use.
import p1.Nope

object Main {
  def main(args: Array[String]): Unit = println(1)
}
