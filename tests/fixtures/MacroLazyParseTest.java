import java.lang.management.ManagementFactory;

public final class MacroLazyParseTest {
    public static void main(String[] args) throws Exception {
        int count = 20000;
        String branch = "(ty \"example.deep.SomeType\" (src 1234567))";
        String packet = "(a " + branch.repeat(count) + ")";
        for (int i = 0; i < 100; i++) ScalaRsMacroEngine.Sexp.parse(branch);
        com.sun.management.ThreadMXBean bean =
            (com.sun.management.ThreadMXBean) ManagementFactory.getThreadMXBean();
        bean.setThreadAllocatedMemoryEnabled(true);
        long thread = Thread.currentThread().getId();
        long before = bean.getThreadAllocatedBytes(thread);
        ScalaRsMacroEngine.Sexp parsed = ScalaRsMacroEngine.Sexp.parse(packet);
        long allocated = bean.getThreadAllocatedBytes(thread) - before;
        if (allocated >= count * 400L) {
            throw new AssertionError("unused protocol children allocated " + allocated + " bytes");
        }
        if (parsed.items.size() != count + 1 || !"a".equals(parsed.items.get(0).atom)) {
            throw new AssertionError("incorrect root shape");
        }
        for (int index : new int[] {count, 1, 17, 3, count - 1}) {
            ScalaRsMacroEngine.Sexp node = parsed.items.get(index);
            if (!branch.equals(node.raw()) || !"ty".equals(node.items.get(0).atom)
                    || !"example.deep.SomeType".equals(node.items.get(1).text())
                    || !"1234567".equals(node.items.get(2).items.get(1).text())) {
                throw new AssertionError("incorrect child at " + index);
            }
        }
        int visited = 0;
        for (ScalaRsMacroEngine.Sexp node : parsed.items.subList(1, count + 1)) {
            if (!branch.equals(node.raw())) throw new AssertionError("lost child");
            visited++;
        }
        if (visited != count) throw new AssertionError("incorrect iteration length");
        for (String value : new String[] {"", "plain", "a\nb\tc\rd", "\\\"", "\u65e5\ud83d\ude00"}) {
            String quoted = ScalaRsMacroEngine.Sexp.quote(value);
            ScalaRsMacroEngine.Sexp node = ScalaRsMacroEngine.Sexp.parse("(a " + quoted + ")");
            if (!value.equals(node.items.get(1).text())) throw new AssertionError("lost string");
        }
        for (String bad : new String[] {"(a (ignored \"bad\\q\"))", "(a (ignored", "(a) tail", "(a ))"}) {
            boolean rejected = false;
            try { ScalaRsMacroEngine.Sexp.parse(bad); }
            catch (IllegalArgumentException expected) { rejected = true; }
            if (!rejected) throw new AssertionError("unvisited malformed child accepted: " + bad);
        }
        ScalaRsMacroEngine.Sexp.parseWithLimits("(())", 4, 2);
        boolean rejected = false;
        try { ScalaRsMacroEngine.Sexp.parseWithLimits("((()))", 6, 2); }
        catch (IllegalArgumentException expected) { rejected = true; }
        if (!rejected) throw new AssertionError("unvisited deep child accepted");
        System.out.println("ok");
    }
}
