import asynchook.Generic
object Main {
  def main(args: Array[String]): Unit = {
    println(Generic.value(41))
    println(Generic.optionally { Generic.value(40) + Generic.value(Some(1)) })
  }
}
