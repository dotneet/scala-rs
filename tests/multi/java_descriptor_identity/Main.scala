object Main {
  def main(args: Array[java.lang.String]): Unit = {
    println(DefaultProvider.make().label())
    println(DefaultProvider.value.label())
    println(scala.custom.Provider.make().label())
    println(scala.custom.Provider.value.label())
    println(scala.custom.Provider.string.label())
    println(scala.custom.Provider.function.label())
    println(scala.custom.Provider.accept(scala.custom.Provider.value))
  }
}
