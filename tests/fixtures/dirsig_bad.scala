import dirsig._
object Main { val r = new Resource[Option, String]("ok"); val bad = r.allocated }
