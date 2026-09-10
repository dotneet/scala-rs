object Main {
 def main(args:Array[String]):Unit={
  val a=new namebatch.Api()
  println(a.+); a.- = 7; println(-a); println(a.*); a./ = 12; println(a./)
  a.`field.name`=13;println(a.`field.name`)
  println(a.**()); println(a.**(6))
  println(a.`space name`(1));println(a.`a.b`(1));println(a.`a\\b`(1));println(a.`line\nname`(1))
  println(a.λ(1));println(a.`😀`(1));println(a.quoted(`some param`=1))
  println(a.`$plus`(1));println(a.`$u0020`(1))
  val s:a.^^ = a.text;println(s)
  val literal: "a.b + spaced" = a.literal;println(literal)
  val number: 42 = a.number;println(number)
  val flag: true = a.flag;println(flag)
  val letter: 'x' = a.letter;println(letter)
  println(a.only(a.literal))
  println(a.old)
 }
}
