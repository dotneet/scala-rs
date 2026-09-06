class VSqlRsBase(
    val url: String,
    val user: String = "user",
    val password: String = "password"
) {
  def this() = this("default", "user", "password")
}

object VSqlRsBase
