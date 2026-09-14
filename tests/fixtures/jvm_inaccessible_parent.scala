package jvmaccess

import inaccessible.{ChildApi, VisibleChild}

object Main {
  def main(args: Array[String]): Unit = {
    val child = new VisibleChild
    println(child.value())
    println(child.inherited())
    val api: ChildApi = child
    println(api.inherited())
    println(api.toString())
  }
}
