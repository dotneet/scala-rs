// A validation pipeline over Either with for-comprehensions, fold, and
// accumulation -- the shape a config/DDL validator takes.
object Main {
  case class Cfg(host: String, port: Int, user: String)

  type V[A] = Either[List[String], A]

  def req(m: Map[String, String], k: String): V[String] =
    m.get(k).filter(_.nonEmpty).toRight(List(s"missing $k"))

  def int(s: String, k: String): V[Int] =
    try Right(s.toInt) catch { case _: NumberFormatException => Left(List(s"$k not an int: $s")) }

  def parse(m: Map[String, String]): V[Cfg] =
    for {
      h <- req(m, "host")
      ps <- req(m, "port")
      p <- int(ps, "port")
      u <- req(m, "user")
      _ <- if (p > 0 && p < 65536) Right(()) else Left(List(s"port out of range: $p"))
    } yield Cfg(h, p, u)

  def accumulate(m: Map[String, String]): Either[List[String], Cfg] = {
    val h = req(m, "host"); val u = req(m, "user")
    val p = req(m, "port").flatMap(int(_, "port"))
    val errs = List(h, u, p).collect { case Left(e) => e }.flatten
    if (errs.nonEmpty) Left(errs)
    else Right(Cfg(h.getOrElse(""), p.getOrElse(0), u.getOrElse("")))
  }

  def main(args: Array[String]): Unit = {
    val good = Map("host" -> "db", "port" -> "5432", "user" -> "app")
    println(parse(good))
    println(parse(good - "port"))
    println(parse(good + ("port" -> "abc")))
    println(parse(good + ("port" -> "70000")))
    println(accumulate(Map("port" -> "zz")))
    println(accumulate(good))

    println(parse(good).map(_.port * 2))
    println(parse(good).fold(_.mkString(";"), _.host))
    println(parse(good - "host").fold(_.mkString(";"), _.host))
    println(parse(good).toOption)
    println(parse(good).left.map(_.size))
    println(parse(good - "user").left.map(_.size))
    println(parse(good).getOrElse(Cfg("?", 0, "?")))
    println(parse(good).swap.isRight)
    println(List(good, good - "host").map(parse).partition(_.isRight)._1.size)

    val opts: List[Option[Int]] = List(Some(1), Some(2), None)
    println(opts.foldRight(Option(List.empty[Int]))((o, acc) => for { x <- o; xs <- acc } yield x :: xs))
    println(opts.take(2).foldRight(Option(List.empty[Int]))((o, acc) => for { x <- o; xs <- acc } yield x :: xs))

    println(Either.cond(1 > 0, "y", "n"))
    println(Right(3).map((x: Int) => x + 1))
    println((Left("e"): Either[String, Int]).map(_ + 1))
    println(parse(good).flatMap(c => if (c.user == "app") Right(c.copy(user = "APP")) else Left(List("bad"))))
  }
}
