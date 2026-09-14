import nestedcompanion.*

object Main {
  def read(info: Service.Info): Int = info.value

  def main(args: Array[String]): Unit = {
    println(read(Service.Info(7)))
  }
}
