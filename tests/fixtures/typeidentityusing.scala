import scala.util.Using
import scala.util.Using.Releasable
object Main { class R { def close():Unit=(); def value:Int=42 }; implicit val r:Releasable[R]=_.close(); def main(args:Array[String]):Unit=println(Using.resource(new R)(_.value)) }