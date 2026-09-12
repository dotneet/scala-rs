// A real Java class's statics are the same rule: Java inherits them, Scala puts
// them on the companion object and does not. scalac: "not found: value
// currentThread" / "not found: value sleep".
class Gz2Thread extends Thread {
  def bad: Thread = currentThread()
  def alsoBad(): Unit = sleep(1L)
}
