import dbconfig.{DatabaseConfig, Profile}

object DbConfigForUrlUse {
  val p = new Profile {}
  val objectResult = DatabaseConfig.forURL(p, "x", null, null)
  val intResult = DatabaseConfig.forURL(p, "x", 2, null)

  def main(args: Array[String]): Unit = {
    println(objectResult)
    println(intResult)
  }
}
