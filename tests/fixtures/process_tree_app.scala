object Main {
  def main(args: Array[String]): Unit = {
    val result = new ProcessTreeLib().processTree(1, 7) { (path, tree) => path + ":" + tree }
    println(result)
  }
}
