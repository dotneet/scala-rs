object Main { val bad:scala.util.Try[Int]=scala.util.Success(1).collect{case i => i.toString} }
