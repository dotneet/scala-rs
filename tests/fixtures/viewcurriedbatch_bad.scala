object Main { implicit class Rich(private val s:String) extends AnyVal {def replaceAll(p:String)(f:String=>String):String=f(s)};val bad="abc".replaceAll("b")(1) }
