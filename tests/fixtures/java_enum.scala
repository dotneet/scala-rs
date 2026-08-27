object Main {
  def main(args: Array[String]): Unit = {
    val s = java.lang.Thread.State.NEW
    println(s.toString)
    println(java.lang.Thread.State.valueOf("RUNNABLE").toString)
    println(java.lang.Thread.State.values().length)
    val n = s match {
      case java.lang.Thread.State.NEW => 1
      case java.lang.Thread.State.RUNNABLE => 2
      case _ => 0
    }
    println(n)
    val r = java.lang.Thread.State.RUNNABLE
    val m = r match {
      case java.lang.Thread.State.NEW => 1
      case java.lang.Thread.State.RUNNABLE => 2
      case _ => 0
    }
    println(m)
    println(s.eq(java.lang.Thread.State.NEW))
  }
}
