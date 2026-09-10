object Main { val bad:scala.util.Try[String]=scala.util.Success(1).orElse(scala.util.Success("bad")) }
