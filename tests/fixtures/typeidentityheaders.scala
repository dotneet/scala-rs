import identityheaders._
import identitybase._
class HeaderGood[A <: Base] extends Bounded[A]
object Main {
 def main(args:Array[String]):Unit={
  val concrete:Bounded[Child]=new HeaderGood[Child]
  println(concrete!=null)
 }
}
