object Main { val bad:scala.util.Try[String]=scala.util.Success(1).recover{case _:RuntimeException => "bad"} }
