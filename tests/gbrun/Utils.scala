// gitbucket's pure utilities, run rather than merely compiled.
//
// `StringUtil` and friends are ordinary Scala -- string handling, regexes,
// `Either`, `Try`, pattern matches over `Option` -- so this program is the
// closest thing in this harness to a plain codegen differential over
// gitbucket's own source, with no database and no macro in the way.
import gitbucket.core.util.{ConfigUtil, StringUtil}
import gitbucket.core.view.helpers

object Main {
  def main(args: Array[String]): Unit = {
    println("-- hashing and encoding")
    println(StringUtil.sha1("gitbucket"))
    println(StringUtil.md5("gitbucket"))
    println(StringUtil.base64Encode("hello world".getBytes("UTF-8")))
    println(new String(StringUtil.base64Decode(StringUtil.base64Encode("round".getBytes("UTF-8")))))
    println(StringUtil.pbkdf2_sha256(1000, StringUtil.base64Encode("salt".getBytes("UTF-8")), "pw"))
    println(StringUtil.encodeBlowfish("secret").length > 0)
    println(StringUtil.decodeBlowfish(StringUtil.encodeBlowfish("secret")))

    println("-- urls and escaping")
    println(StringUtil.urlEncode("a b/c?d=e&f"))
    println(StringUtil.urlDecode(StringUtil.urlEncode("a b/c?d=e&f")))
    println(StringUtil.encodeRefName("feature/new thing"))
    println(StringUtil.escapeHtml("<a href=\"x\">&amp;</a>"))

    println("-- splitting and parsing")
    println(StringUtil.splitWords("one  two\tthree").toList)
    println(List("12", "-3", "x", "", "2147483648").map(StringUtil.isInteger))
    println(StringUtil.extractIssueId("fixes #12 and #34, not #x").toList)
    println(StringUtil.extractCloseId("close #9, closes #10, fix #11").toList)

    println("-- line separators")
    println(StringUtil.convertLineSeparator("a\r\nb\rc\nd", "\n").replace("\n", "<LF>"))
    println(StringUtil.convertLineSeparator("a\nb", "\r\n").replace("\r\n", "<CRLF>"))
    println(StringUtil.appendNewLine("x", "\n").replace("\n", "<LF>"))
    println(StringUtil.appendNewLine("x\n", "\n").replace("\n", "<LF>"))

    println("-- encodings")
    println(StringUtil.convertFromByteArray("日本語".getBytes("UTF-8")))
    println(StringUtil.hasUtf8Bom(StringUtil.Utf8Bom ++ "x".getBytes("UTF-8")))
    println(StringUtil.hasUtf8Bom("x".getBytes("UTF-8")))

    println("-- ConfigUtil")
    println(ConfigUtil.getConfigValue[String]("gbrun.does.not.exist"))
    println(ConfigUtil.getSystemProperty[String]("gbrun.no.such.property"))
    println(ConfigUtil.getEnvironmentVariable[String]("GBRUN_NO_SUCH_VARIABLE"))

    println("-- view helpers (the ones that need no request Context)")
    val epoch = new java.util.Date(0L)
    println(helpers.date(epoch))
    println(helpers.hashDate(epoch))
    println(helpers.datetimeRFC3339(epoch))
    println(helpers.urlEncode("a b"))
    println(helpers.urlEncode(Some("a b")))
    println(helpers.urlEncode(None))
    println(helpers.encodeRefName("feature/x y"))
    println(helpers.plural(1, "issue"))
    println(helpers.plural(2, "issue"))
    println(helpers.plural(2, "entry", "entries"))
    println(helpers.isRenderable("README.md"))
    println(helpers.isRenderable("Main.scala"))
    println(helpers.removeHtml(play.twirl.api.Html("<b>bold</b> text")).body)
  }
}
