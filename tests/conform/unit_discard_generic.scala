// A generic call whose value a `Unit` method discards. nsc's `methTypeArgs`
// lets an expected `Unit` constrain nothing (`isWeaklyCompatible`), so the
// type parameters come from the arguments and the result is then discarded
// (gitbucket's `serverSession.setAttribute(authTypeSessionKey, authType)`).
trait AttributeKey[T]
class Session {
  var log = List.empty[String]
  def setAttribute[T](key: AttributeKey[T], value: T): T = { log = s"set $value" :: log; value }
  def get[T](key: AttributeKey[T]): Option[T] = None
}
sealed trait AuthType
object AuthType { case object Key extends AuthType; case object Deploy extends AuthType }
object Main {
  val key = new AttributeKey[AuthType] {}
  def put(s: Session, a: AuthType): Unit = s.setAttribute(key, a)
  def twice[T](x: T)(f: T => T): T = f(f(x))
  def bump(n: Int): Unit = twice(n)(_ + 1)
  def pick[T](xs: List[T]): T = xs.head
  def first(xs: List[String]): Unit = pick(xs)
  def nothing[T]: T = throw new RuntimeException("nothing")
  def main(args: Array[String]): Unit = {
    val s = new Session
    put(s, AuthType.Key)
    put(s, AuthType.Deploy)
    println(s.log.reverse)
    bump(1)
    first(List("a"))
    val u: Unit = s.setAttribute(key, AuthType.Key)
    println(u)
    println(s.log.size)
    println(scala.util.Try { val v: Unit = nothing; v }.isFailure)
  }
}
