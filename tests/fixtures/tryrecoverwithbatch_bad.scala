object Main { val bad:scala.util.Try[String]=scala.util.Success(1).recoverWith{case _:RuntimeException => scala.util.Success("bad")} }
