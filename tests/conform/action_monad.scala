// A DBIO-shaped action monad: a sealed hierarchy of actions built with
// map/flatMap/andThen, then interpreted by a fold over a session. This is the
// core shape of slick's `DBIOAction`.
object Main {
  class Session(val name: String) {
    private var store = scala.collection.mutable.Map.empty[String, Int]
    var ops = 0
    def put(k: String, v: Int): Unit = { ops += 1; store(k) = v }
    def get(k: String): Option[Int] = { ops += 1; store.get(k) }
    def keys: List[String] = store.keys.toList.sorted
  }

  sealed trait Act[+A] {
    def map[B](f: A => B): Act[B] = FlatMap(this, (a: A) => Pure(f(a)))
    def flatMap[B](f: A => Act[B]): Act[B] = FlatMap(this, f)
    def andThen[B](next: => Act[B]): Act[B] = FlatMap(this, (_: A) => next)
    def zip[B](other: Act[B]): Act[(A, B)] = for { a <- this; b <- other } yield (a, b)
  }
  case class Pure[A](a: A) extends Act[A]
  case class Run[A](f: Session => A) extends Act[A]
  case class FlatMap[A, B](src: Act[A], k: A => Act[B]) extends Act[B]
  case class Failed(msg: String) extends Act[Nothing]

  def exec[A](act: Act[A], s: Session): Either[String, A] = act match {
    case Pure(a) => Right(a)
    case Run(f) => Right(f(s))
    case Failed(m) => Left(m)
    case fm: FlatMap[a, A] => exec(fm.src, s) match {
      case Left(m) => Left(m)
      case Right(v) => exec(fm.k(v), s)
    }
  }

  def put(k: String, v: Int): Act[Unit] = Run(_.put(k, v))
  def get(k: String): Act[Option[Int]] = Run(_.get(k))
  def seqAll[A](as: List[Act[A]]): Act[List[A]] =
    as.foldRight(Pure(List.empty[A]): Act[List[A]])((h, t) => h.flatMap(a => t.map(a :: _)))

  def main(args: Array[String]): Unit = {
    val s = new Session("s1")
    val prog = for {
      _ <- put("a", 1)
      _ <- put("b", 2)
      a <- get("a")
      b <- get("b")
      _ <- put("sum", a.getOrElse(0) + b.getOrElse(0))
      sum <- get("sum")
    } yield sum.getOrElse(-1)
    println(exec(prog, s))
    println(s.keys)
    println(s.ops)

    println(exec(put("x", 9).andThen(get("x")), s))
    println(exec(Failed("nope").andThen(put("y", 1)), s))
    println(exec(get("zz").map(_.isDefined), s))
    println(exec(get("a").zip(get("b")), s))
    println(exec(seqAll(List(put("p", 1), put("q", 2))), s))
    println(exec(seqAll((1 to 3).toList.map(i => get("a").map(_.map(_ + i)))), s))
    println(s.keys)

    val chain = (1 to 4).foldLeft(Pure(0): Act[Int])((acc, i) => acc.flatMap(n => Run(_ => n + i)))
    println(exec(chain, s))
    println(exec(chain.flatMap(n => if (n > 5) Failed("too big " + n) else Pure(n)), s))
  }
}
