import org.scalatra.forms._
import org.scalatra.i18n.Messages
object Main {
 def form = mapping("url" -> text(), "events" -> events, "ctype" -> text(), "token" -> optional(text()))((url,events,ctype,token) => (url,events.toList.sorted.mkString(","),ctype,token))
 def events = new ValueType[Set[String]] {
 def convert(name:String, params:Map[String,Seq[String]], messages:Messages):Set[String] = params.getOrElse(name,Seq.empty[String]).toSet
 def validate(name:String, params:Map[String,Seq[String]], messages:Messages):Seq[(String,String)] = if (convert(name,params,messages).isEmpty) Seq(name -> "empty") else Nil
}
 def main(args:Array[String]):Unit={
   val params = Map("url" -> Seq("url"), "events" -> Seq("push","pull"), "ctype" -> Seq("json"), "token" -> Seq("secret"))
   val messages = Messages()
   println(form.convert("",params,messages))
   println(form.validate("",params,messages).isEmpty)
 }
}
