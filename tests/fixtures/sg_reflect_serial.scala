object CheckSerial {
  def main(args: Array[String]): Unit = {
    val mirror = scala.reflect.runtime.universe.runtimeMirror(getClass.getClassLoader)
    println(mirror.staticClass("sg.Ser").toType.typeSymbol.fullName)
  }
}
