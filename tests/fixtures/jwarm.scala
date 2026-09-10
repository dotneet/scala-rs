import jwarm._
class Impl extends Api[String] with Marker {
  def echo(value: String): String = value + "!"
  def mark(): String = "marker"
}
object Main {
  def main(args: Array[String]): Unit = {
    val impl = new Impl
    val api: Api[String] = impl
    val marker: Marker = impl
    println(api.echo("hello"))
    println(marker.mark())
  }
}
