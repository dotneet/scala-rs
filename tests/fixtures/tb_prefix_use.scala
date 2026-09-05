// The call sites for `tb_prefix_impl.scala`. Compiled in a second run, with
// the first run's output on the classpath, because expanding a macro means
// loading its implementation's class file.
import scala.language.experimental.macros

object TbMacros {
  def show: String = macro TbPrefixImpls.show
}

object Main {
  def main(args: Array[String]): Unit = {
    println(TbMacros.show)
  }
}
