// scalac: illegal inheritance from final class F
object Main {
  final class F
  class G extends F
  def main(args: Array[String]): Unit = println(new G)
}
