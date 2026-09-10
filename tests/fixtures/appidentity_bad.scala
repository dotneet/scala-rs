package custom { trait App }
object Main { val wrong: scala.App = new custom.App {} }
