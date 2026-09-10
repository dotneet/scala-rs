trait W { implicit def algebra: String }
object Main { val w = new W { val algebra = 3 } }

