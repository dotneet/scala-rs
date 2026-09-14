trait MiniMonad[F[_]] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

sealed abstract class MiniAndThen[-T, +R] extends (T => R)

object MiniAndThen {
  def apply[A, B](f: A => B): MiniAndThen[A, B] =
    new MiniAndThen[A, B] { def apply(a: A): B = f(a) }

  def instance[T]: MiniMonad[({ type L[X] = MiniAndThen[T, X] })#L] =
    new MiniMonad[({ type L[X] = MiniAndThen[T, X] })#L] {
      private[this] val fn1: MiniMonad[({ type L[X] = T => X })#L] = new MiniMonad[({ type L[X] = T => X })#L] {
        def flatMap[A, B](fa: T => A)(f: A => T => B): T => B =
          (t: T) => f(fa(t))(t)
      }

      def flatMap[A, B](fa: MiniAndThen[T, A])(f: A => MiniAndThen[T, B]): MiniAndThen[T, B] = {
        val plain: T => B = fn1.flatMap(fa)(f)
        MiniAndThen(plain)
      }
    }
}

object Main {
  def main(args: Array[String]): Unit = {
    val m = MiniAndThen.instance[Int]
    val f = MiniAndThen((x: Int) => x + 1)
    val result = m.flatMap(f)((x: Int) => MiniAndThen((y: Int) => x + y))
    println("ok")
  }
}
