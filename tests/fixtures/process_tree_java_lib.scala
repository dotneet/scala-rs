import java.util.ArrayList

object ProcessTreeJavaLib {
  def processTree[T](git: ArrayList[String], id: ArrayList[String])(
      f: (String, ArrayList[String]) => T
  ): Seq[T] = Seq(f("tree", id))
}
