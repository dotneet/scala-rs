// implicit が見つからない／メンバが見えない 7 根をまとめた fixture。
//
// 1. 適用済み抽象型メンバが自分の上限に適合しない（`CT[U] <: TT[U]`）
// 2. コンテキスト境界の evidence 型が self type 越しに展開されない
// 3. コンパニオン object の `protected` メンバをコンパニオン側から読む
// 4. 入れ子の `private[pkg] object`
// 5. 匿名クラスの self alias（`new T { base => … }`）
// 6. コンストラクタパターンの関数位置で、非 stable な `def` が抽出子を隠す
// 7. Java の `Object` 戻り値が `Any` になって `eq`/`ne` が無い
// 8. `scala.collection.Map` の `get`/`contains`/`getOrElse`/`apply`

package implfind {

  // ---- 1 / 2: ケーキの抽象型メンバと文脈境界 -----------------------------
  trait TT[T] { def name: String }
  trait BB[T] extends TT[T]

  trait Comp { self: Prof =>
    type CT[T] <: TT[T]
    type BCT[T] <: CT[T] with BB[T]
  }

  trait Prof extends Comp { self: Prof =>
    // 上限越しの探索: 候補は `BCT[U]` の evidence だけ。
    def viaBound[U: BCT](u: U): String = implicitly[TT[U]].name
    // 同じ名前で要求する側。self type で具体化された別名になる。
    def viaSelf[U: BCT](u: U): String = implicitly[BCT[U]].name
  }

  // self type が具体プロファイルを指すコンポーネント。ここで書かれた
  // `[U : BCT]` の evidence は self type 越しに `JT[U] with BB[U]` に
  // なっていなければならない（本体の `implicitly[BCT[U]]` はそうなる）。
  trait JComp extends Comp { self: JProf =>
    def viaComponent[U: BCT](u: U): String = implicitly[BCT[U]].name
    def viaComponentJT[U: BCT](u: U): String = implicitly[JT[U]].name
  }

  trait JProf extends Prof with JComp {
    type CT[T] = JT[T]
    type BCT[T] = JT[T] with BB[T]
  }

  trait JT[T] extends TT[T]

  object Cake extends JProf

  // ---- 3: コンパニオン object の protected --------------------------------
  trait Prot {
    def viaTrait: Int = Prot.hidden
  }
  object Prot {
    protected val hidden: Int = 7
  }

  class ProtC {
    def viaClass: Int = ProtC.hidden
  }
  object ProtC {
    protected val hidden: Int = 11
  }

  // ---- 4: 入れ子の private[pkg] object ------------------------------------
  object Outer {
    private[implfind] object Inner { val v: Int = 13 }
    private[implfind] class InnerC { val v: Int = 17 }
  }

  class UsesInner {
    def a: Int = Outer.Inner.v
    def b: Int = new Outer.InnerC().v
  }

  // ---- 5: 匿名クラスの self alias -----------------------------------------
  trait Tag {
    def label: String
    def tagged(i: Int): Tag
  }

  object Anon {
    def run: String = {
      val outer = new Tag { base =>
        def label = "base"
        def tagged(i: Int): Tag = new Tag {
          def label = "ref" + i
          def tagged(j: Int): Tag = base.tagged(j)
        }
      }
      outer.tagged(1).tagged(2).label
    }
  }

  // ---- 6: 記号名の抽出子と同名の非 stable な def ---------------------------
  class Nd(val s: String) {
    final def :@(t: Int): Nd = new Nd(s + t)
  }

  object NdOps {
    object :@ {
      def unapply(n: Nd): Option[(Nd, Int)] = Some((n, n.s.length))
    }
  }

  import NdOps._

  class Sub(s: String) extends Nd(s) {
    // `:@` はここでは継承した *メソッド* でもあるが、構成子パターンの
    // 関数位置ではメソッドは候補にならない。
    def viaVal: Int = {
      val _ :@ n = (new Nd("abc")): @unchecked
      n
    }
    def viaCase: Int = (new Nd("abcd")) match {
      case _ :@ n => n
    }
    def viaMethod: String = (this :@ 5).s
  }
}

object Main {
  import implfind._

  implicit val jtInt: JT[Int] with BB[Int] = new JT[Int] with BB[Int] {
    def name = "jt-int"
  }

  // ---- 8: collection.Map --------------------------------------------------
  def viaCollMap(m: scala.collection.Map[String, Int]): String =
    s"${m.contains("a")} ${m("a")} ${m.get("b")} ${m.getOrElse("b", 9)}"

  def main(args: Array[String]): Unit = {
    println(Cake.viaBound(1))
    println(Cake.viaSelf(1))
    println(Cake.viaComponent(1))
    println(Cake.viaComponentJT(1))
    println(new Prot {}.viaTrait)
    println(new ProtC().viaClass)
    println(new UsesInner().a)
    println(new UsesInner().b)
    println(Anon.run)
    println(new Sub("z").viaVal)
    println(new Sub("z").viaCase)
    println(new Sub("z").viaMethod)

    // ---- 7: Java の Object 戻り値 ----------------------------------------
    val props = new java.util.Properties()
    props.put("k", "v")
    println(props.get("k") ne null)
    println(props.get("nope") eq null)

    println(viaCollMap(Map("a" -> 1)))
  }
}
