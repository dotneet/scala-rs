object Main { def main(args: Array[String]): Unit = { val s: String = new Child().value; println(s) } }
class Child extends Base { def value = "ok" }
abstract class Base { def value: Any }
