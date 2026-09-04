// An XML literal whose attribute names are Scala keywords. nsc reads XML
// names with the XML scanner, where `class` is an ordinary name; gitbucket's
// templates are full of `<span class="simplified-path">`.
object Main {
  def main(args: Array[String]): Unit = {
    val cls = "simplified-path"
    val e = <span class={cls} type="text" for="name" val="v">hello</span>
    println(e.toString)
    val f = <a href="/x" if="no"><b class="c">deep</b></a>
    println(f.toString)
  }
}
