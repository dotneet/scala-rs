import java.util.ArrayList

object ProcessTreeJavaMain {
  def main(args: Array[String]): Unit = {
    val tree = new ArrayList[String]()
    tree.add("entry")
    val result = ProcessTreeJavaLib.processTree(tree, tree) { (path, parsed) =>
      path + ":" + parsed.size
    }
    println(result.head)
  }
}
