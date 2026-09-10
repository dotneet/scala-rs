object Main { val bad:scala.util.Try[Int]=scala.util.Success(1).transform((i:Int)=>scala.util.Success(i.toString),(e:Throwable)=>scala.util.Success("bad")) }
