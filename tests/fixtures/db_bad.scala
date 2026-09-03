// agent/dbio: 親コンストラクタの名前付き引数は「通す」だけではなく、
// 名前が違えば従来どおり診断されなければならない。
abstract class Act(_name: String, statement: String)

class Wrong extends Act(_name = "Wrong", stmt = "s")

object Main {
  def main(args: Array[String]): Unit = println(new Wrong)
}
