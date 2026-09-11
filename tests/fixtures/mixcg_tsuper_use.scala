// Uses `mixcg_tsuper/MixcgTs.scala`.
import mixcgts._
class C extends B0 with T
class CU extends B0 with U with T
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C
    println(new c.In().x)
    val cu = new CU
    println(new cu.In().x + " " + cu.f)
  }
}
