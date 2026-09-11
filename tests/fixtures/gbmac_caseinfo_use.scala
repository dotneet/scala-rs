// slick's `mapToImpl` opening -- `isCaseClass`, then every case accessor's
// `typeSignature` -- on case classes this run is compiling
// (`gbm_impl.scala`'s `caseInfoImpl`). This call was the refusal at the head
// of `gbm_bad.scala` until `docs/macros.md` §7.25: such a class reached the
// engine as a placeholder carrying only its name. Real scalac 2.13.16 prints
//
//     Nothing v:Int
//     Nothing id:Int,name:String,tags:List[String]
//     Nothing when:java.util.Date,note:Option[String]
//
// and so must scala-rs (`crates/cli/tests/gbmac.rs`).
import scala.language.experimental.macros
import scala.reflect.ClassTag

case class LocalRow(v: Int)
case class Wider(id: Int, name: String, tags: List[String])

object Holder {
  case class Nested(when: java.util.Date, note: Option[String])
}

object CaseInfoUse {
  def caseInfo[R](implicit ct: ClassTag[R]): String = macro GbmImpl.caseInfoImpl[R]
}

object Main {
  def main(args: Array[String]): Unit = {
    println(CaseInfoUse.caseInfo[LocalRow])
    println(CaseInfoUse.caseInfo[Wider])
    println(CaseInfoUse.caseInfo[Holder.Nested])
  }
}
