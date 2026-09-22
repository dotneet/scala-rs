object Main {
  def main(args: Array[String]): Unit = println(ConstructorMacro.inspect[ConstructorBox[ConstructorAliases.Identity]])
}
