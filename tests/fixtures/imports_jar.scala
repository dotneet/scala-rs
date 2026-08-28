// Imports whose packages exist only in the classpath jar: a nested package,
// a package object (`scala.math`) and its members, wildcards and renames.
import scala.collection.mutable.*
import scala.math.*
import scala.util.control.NonFatal
import scala.collection.immutable.{ListMap => LM}

object Main {
  def main(args: Array[String]): Unit = {
    val b = new ArrayBuffer[Int]()
    b += 7
    println(b.head)
    val s = new StringBuilder
    s.append("ok")
    println(s.toString)
    println(Pi)
    val e = LM.empty[String, Int]
    println(e.size)
    println(NonFatal(new RuntimeException("boom")))
  }
}
