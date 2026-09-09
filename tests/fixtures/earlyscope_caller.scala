class Parent(r: Runnable) { def run(): Unit = r.run() }
object SmallCaller extends Parent(new Runnable {
  def run(): Unit = println(demo.Directory.conf.getName)
}) {
  def main(args: Array[String]): Unit = run()
}
