object Main {
  def main(args: Array[String]): Unit = {
    // No `(String, String)` constructor on RuntimeException.
    println(new RuntimeException("boom", "not a Throwable"))
  }
}
