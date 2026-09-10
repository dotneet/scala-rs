class Cell[T](var value:T) {def set(x:T):T={value=x;value}}
class IntCell extends Cell[Int](1) {def bump():Int={value=7;value}}
object Store extends Cell[Int](2)
object Main {def main(args:Array[String]):Unit={
 println(new Cell[String]("a").set("b"));println(new IntCell().bump())
 import Store._;value=9;println(value)
}}
