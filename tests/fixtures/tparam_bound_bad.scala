trait Named { def name: String }
object Main {
  def greet[A <: Named](x: A): String = x.nosuchmember
}
