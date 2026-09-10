object Main { def main(args:Array[String]):Unit={
 val patch="diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n"
 val parsed=new org.eclipse.jgit.patch.Patch()
 parsed.parse(new java.io.ByteArrayInputStream(patch.getBytes("UTF-8")))
 print(gitbucket.core.util.PatchUtil.apply("old\n",patch,parsed.getFiles.get(0)))
}}
