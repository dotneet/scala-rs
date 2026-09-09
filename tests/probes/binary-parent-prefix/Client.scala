import probe.O.Alias
class Child extends Alias(7)
object Main { def main(args: Array[String]): Unit = println(new Child().n) }
