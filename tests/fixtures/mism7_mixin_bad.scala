// Two classes in one template: scalac reports
// "class B needs to be a trait to be mixed in".
class A
class B
class C extends A with B
object Main {
  def main(args: Array[String]): Unit = ()
}
