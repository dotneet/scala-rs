trait FK[F[_],G[_]] {self=>def apply[A](a:F[A]):G[A];def wrong[A](a:F[A]):G[String]=self(a)}
