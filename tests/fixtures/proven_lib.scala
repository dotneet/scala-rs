package boundshape
trait Level
trait Flat extends Level
class Rep[T](val value: T)
trait Shape[L <: Level, A, B, P] { def apply(a: A): B }
object Shape { implicit def repShape[L <: Level, T]: Shape[L, Rep[T], T, Rep[T]] = new Shape[L, Rep[T], T, Rep[T]] { def apply(a: Rep[T]): T = a.value } }
class Proven[B](val value: B)
object Proven { implicit def prove[A, B](a: A)(implicit shape: Shape[_ <: Flat, A, B, _]): Proven[B] = new Proven(shape(a)) }
