import java.lang.reflect.Method;

public final class MacroMethodListCacheTest {
    public static final class Flags {
        long current = 1;
        public long PUBLIC() { return current; }
        public long OTHER() { return 2; }
        public long lowercase() { return 4; }
    }

    public static final class Universe {
        final Flags flags = new Flags();
        public Flags Flag() { return flags; }
    }

    public static final class Mods {
        public long flags() { return 26; }
        public Object privateWithin() { return null; }
        public Object annotations() { return null; }
    }

    public static void main(String[] args) throws Exception {
        Universe universe = new Universe();
        ScalaRsMacroEngine.universe = universe;
        ScalaRsMacroEngine.methodsCache.clear();
        if (ScalaRsMacroEngine.flagValue("PUBLIC") != 1) throw new AssertionError("wrong flag");
        Method[] cached = ScalaRsMacroEngine.methodsCache.get(Flags.class);
        if (cached == null) throw new AssertionError("flag lookup did not reuse method metadata");
        universe.flags.current = 8;
        if (ScalaRsMacroEngine.flagValue("PUBLIC") != 8) throw new AssertionError("cached flag value");
        if (cached != ScalaRsMacroEngine.methodsCache.get(Flags.class)) {
            throw new AssertionError("method metadata was replaced");
        }
        try {
            ScalaRsMacroEngine.flagValue("MISSING");
            throw new AssertionError("unknown flag accepted");
        } catch (IllegalStateException expected) {
            if (!expected.getMessage().contains("MISSING")) throw expected;
        }

        StringBuilder expected = new StringBuilder("(mods (f");
        long known = 0;
        for (Method method : Flags.class.getMethods()) {
            if (method.getParameterCount() != 0 || method.getReturnType() != long.class) continue;
            String name = method.getName();
            if (!name.equals(name.toUpperCase()) || name.isEmpty()) continue;
            long value = ((Number) method.invoke(universe.flags)).longValue();
            if (value != 0 && (26 & value) == value) {
                known |= value;
                expected.append(' ').append(ScalaRsMacroEngine.Sexp.quote(name));
            }
        }
        expected.append(") (rest ")
            .append(ScalaRsMacroEngine.Sexp.quote(Long.toHexString(26 & ~known)))
            .append(") \"\" (o \"null\"))");
        ScalaRsMacroEngine.methodsCache.remove(Flags.class);
        StringBuilder actual = new StringBuilder();
        ScalaRsMacroEngine.serMods(new Mods(), actual);
        if (!expected.toString().equals(actual.toString())) throw new AssertionError(actual);
        if (!ScalaRsMacroEngine.methodsCache.containsKey(Flags.class)) {
            throw new AssertionError("modifier serialization did not reuse method metadata");
        }
        System.out.println("ok");
    }
}
