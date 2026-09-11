// Overriding a Java method whose parameter is `Map<String, Object>`: the
// Scala override may write the `Object` as `AnyRef`, `Any` or `Object`
// (gitbucket's `GitBucketCoreModule` writes `java.util.Map[String, AnyRef]`).
// Needs `tests/fixtures/gbmisc_java/GbmiscMig.java` on the class path.
object Main {
  def main(args: Array[String]): Unit = {
    val m1 = new GbmiscMig() {
      override def migrate(moduleId: String, version: String, context: java.util.Map[String, AnyRef]): Unit =
        println("m1 " + moduleId + version + context)
    }
    val m2 = new GbmiscMig {
      def migrate(moduleId: String, version: String, context: java.util.Map[String, Any]): Unit =
        println("m2 " + moduleId + version + context)
    }
    val m3 = new GbmiscMig {
      def migrate(moduleId: String, version: String, context: java.util.Map[String, Object]): Unit =
        println("m3 " + moduleId)
    }
    val ctx = new java.util.HashMap[String, AnyRef]()
    m1.migrate("a", "1", ctx)
    m2.migrate("b", "2", new java.util.HashMap[String, Any]())
    m3.migrate("c", "3", ctx)
  }
}
