package demo
import java.io.File
object Directory {
  val home = new File(".").getAbsolutePath
  val conf = new File(home, "probe")
}
