object Main { val bad:scala.util.Try[Int]=scala.util.Success(1).flatMap((i:Int)=>scala.util.Success(i.toString)) }
