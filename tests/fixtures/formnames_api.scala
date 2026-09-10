package namebatch
class Api(val + : Int = 3, var - : Int = 4) {
 val * : Int = 9
 var / : Int = 10
 var `field.name`:Int=11
 def **(x:Int=5):Int=this.+ + x
 def unary_- :Int= -this.-
 def `space name`(x:Int):Int=x+1
 def `a.b`(x:Int):Int=x+2
 def `a\\b`(x:Int):Int=x+3
 def `line\nname`(x:Int):Int=x+4
 def λ(x:Int):Int=x+5
 def `😀`(x:Int):Int=x+6
 def quoted(`some param`:Int):Int=`some param`+7
 def `$plus`(x:Int):Int=x+8
 def `$u0020`(x:Int):Int=x+9
 type ^^ = String
 def text: ^^ = "ok"
 final val literal: "a.b + spaced" = "a.b + spaced"
 final val number: 42 = 42
 final val flag: true = true
 final val letter: 'x' = 'x'
 def only(v: "a.b + spaced"): Int = 12
 @deprecated("a.b + spaced", "1") def old:Int=42
}
