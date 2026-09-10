object Main { val bad=Option(1).collect[String] {case s:String => s} }
