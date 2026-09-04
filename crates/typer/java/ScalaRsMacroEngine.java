import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

/**
 * The JVM half of scala-rs's def-macro expander (`docs/macros.md` §2.2, §5).
 *
 * nsc runs a macro implementation for real: it loads the implementation class
 * with a class loader built from the macro classpath and calls it through Java
 * reflection, handing it a `scala.reflect.macros.blackbox.Context`. scala-rs is
 * not on the JVM, so this process is that half. It:
 *
 *   1. reads one request per line from stdin,
 *   2. builds the argument trees and type tags inside
 *      `scala.reflect.runtime.universe`,
 *   3. calls the implementation reflectively through a `Context` proxy,
 *   4. writes the returned tree back as one line.
 *
 * Everything about Scala is reached by reflection, so this file compiles with
 * plain `javac` and no Scala jar on the compile classpath; the jars only have
 * to be on the *runtime* classpath, which is the macro classpath scala-rs
 * passes in.
 *
 * Nothing here guesses. A `Context` member that is not implemented throws
 * rather than returning null, an unknown node kind is an error reply, and
 * scala-rs turns every error reply into a compile diagnostic.
 */
public final class ScalaRsMacroEngine {
    static Object universe;
    static Object mirror;
    static ClassLoader macroCl;
    /** `c.freshName` counter, like nsc's per-run one. */
    static int fresh = 0;
    /**
     * What `c.compilerSettings` returns: the compiler's own command line, as
     * scala-rs rebuilt it (`crates/driver/src/lib.rs`, `compiler_settings`).
     * A macro that gates on a flag reads it here -- `scala.async`'s
     * `asyncImpl` aborts unless it contains `-Xasync`.
     */
    static List<String> compilerSettings = new ArrayList<>();

    public static void main(String[] args) throws Exception {
        PrintStream out = new PrintStream(System.out, true, "UTF-8");
        BufferedReader in =
            new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        macroCl = ScalaRsMacroEngine.class.getClassLoader();
        try {
            Class<?> pkg = Class.forName("scala.reflect.runtime.package$", true, macroCl);
            Object mod = pkg.getField("MODULE$").get(null);
            universe = pkg.getMethod("universe").invoke(mod);
            mirror = find(universe.getClass(), "runtimeMirror", 1)
                .invoke(universe, macroCl);
        } catch (Throwable t) {
            out.println(err("cannot start the macro engine: " + describe(t)));
            return;
        }
        out.println("(ready)");
        String line;
        while ((line = in.readLine()) != null) {
            if (line.isEmpty()) {
                continue;
            }
            String reply;
            try {
                reply = handle(line);
            } catch (Throwable t) {
                reply = err(describe(t));
            }
            out.println(reply);
        }
    }

    // ---------------------------------------------------------------- request

    static String handle(String line) throws Exception {
        Sexp req = Sexp.parse(line);
        if (!req.isList() || req.items.isEmpty()) {
            return err("malformed request");
        }
        String head = req.items.get(0).atom;
        if ("quit".equals(head)) {
            System.exit(0);
        }
        if (!"expand".equals(head)) {
            return err("unknown request " + head);
        }
        String className = req.items.get(1).text();
        String methodName = req.items.get(2).text();
        Sexp argss = req.field("argss");
        Sexp tags = req.field("tags");
        compilerSettings = new ArrayList<>();
        for (Sexp x : req.field("settings").items.subList(1, req.field("settings").items.size())) {
            compilerSettings.add(x.text());
        }

        Class<?> implCls;
        try {
            implCls = Class.forName(className, true, macroCl);
        } catch (ClassNotFoundException e) {
            return err("macro implementation class " + className
                + " is not on the macro classpath (nsc requires the implementation to have "
                + "been compiled by an earlier run)");
        }
        Object receiver = implCls.getField("MODULE$").get(null);
        Method impl = null;
        for (Method m : implCls.getMethods()) {
            if (m.getName().equals(methodName)) {
                impl = m;
                break;
            }
        }
        if (impl == null) {
            return err("no method " + methodName + " on " + className);
        }

        Ctx handler = new Ctx();
        // `c.prefix`: the receiver of the macro application, or the reason
        // there is none -- which is raised only if the implementation reads it.
        Sexp pfx = req.field("prefix").items.get(1);
        if (pfx.isList() && "no".equals(pfx.items.get(0).atom)) {
            handler.prefixWhy = pfx.items.get(1).text();
        } else {
            handler.prefixTree = buildTree(pfx);
        }
        // `c.macroApplication`: the call as written, carried the same way.
        Sexp app = req.field("app").items.get(1);
        if (app.isList() && "no".equals(app.items.get(0).atom)) {
            handler.appWhy = app.items.get(1).text();
        } else {
            handler.appTree = buildTree(app);
        }

        Object ctx = Proxy.newProxyInstance(
            ScalaRsMacroEngine.class.getClassLoader(),
            new Class<?>[]{Class.forName("scala.reflect.macros.blackbox.Context", true, macroCl)},
            handler);

        // 2.11 onwards an implementation may take a raw `c.Tree` instead of a
        // `c.Expr[T]`, and slick's `mapToImpl` does. Which one is wanted is
        // read off the implementation's *source* signature by scala-rs and
        // sent along, because the erased signature does not always say: an
        // abstract type member erases to `Object` in class files scala-rs
        // itself writes. Handing an `Expr` to a `Tree` parameter is an
        // `IllegalArgumentException` from `Method.invoke`, not a diagnostic.
        List<Object> argv = new ArrayList<>();
        argv.add(ctx);
        for (Sexp clause : argss.items.subList(1, argss.items.size())) {
            for (Sexp a : clause.items.subList(1, clause.items.size())) {
                boolean asExpr = "expr".equals(a.items.get(1).atom);
                Object tree = buildTree(a.items.get(2));
                argv.add(asExpr ? mkExpr(tree, buildTag(a.items.get(3))) : tree);
            }
        }
        for (Sexp t : tags.items.subList(1, tags.items.size())) {
            argv.add(buildTag(t));
        }
        if (argv.size() != impl.getParameterCount()) {
            return err("macro implementation " + className + "." + methodName + " takes "
                + impl.getParameterCount() + " arguments, the call site supplies " + argv.size());
        }

        Object result;
        try {
            result = impl.invoke(receiver, argv.toArray());
        } catch (InvocationTargetException e) {
            Throwable cause = e.getCause();
            if (cause instanceof Abort) {
                return "(abort " + Sexp.quote(cause.getMessage()) + ")";
            }
            return err("the macro implementation threw " + describe(cause));
        }
        Object tree = result;
        Class<?> exprCls = Class.forName("scala.reflect.api.Exprs$Expr", true, macroCl);
        if (exprCls.isInstance(result)) {
            tree = find(result.getClass(), "tree", 0).invoke(result);
        }
        StringBuilder sb = new StringBuilder("(ok ");
        ser(tree, sb);
        sb.append(')');
        return sb.toString();
    }

    // ------------------------------------------------------- building trees

    /** A tree the request describes, built in the runtime universe. */
    static Object buildTree(Sexp s) throws Exception {
        if (!s.isList() || s.items.isEmpty() || !"t".equals(s.items.get(0).atom)) {
            throw new IllegalArgumentException("malformed tree: " + s);
        }
        String kind = s.items.get(1).text();
        List<Sexp> kids = s.items.subList(3, s.items.size());
        switch (kind) {
            case "EmptyTree":
                return call(universe, "EmptyTree", 0);
            case "Literal":
                return call(companion("Literal"), "apply", 1, constant(kids.get(0)));
            case "Ident":
                return call(companion("Ident"), "apply", 1, termName(nameOf(kids.get(0))));
            case "This":
                return call(companion("This"), "apply", 1, typeName(nameOf(kids.get(0))));
            case "Select":
                return call(companion("Select"), "apply", 2,
                    buildTree(kids.get(0)), termName(nameOf(kids.get(1))));
            case "Apply": {
                Object fun = buildTree(kids.get(0));
                List<Object> as = new ArrayList<>();
                for (Sexp k : kids.get(1).items.subList(1, kids.get(1).items.size())) {
                    as.add(buildTree(k));
                }
                return call(companion("Apply"), "apply", 2, fun, list(as));
            }
            default:
                throw new IllegalArgumentException(
                    "scala-rs cannot hand a " + kind + " to a macro implementation");
        }
    }

    /** The text of an `(n term "x")` node. */
    static String nameOf(Sexp s) {
        if (s.isList() && s.items.size() == 3) {
            return s.items.get(2).text();
        }
        throw new IllegalArgumentException("malformed name: " + s);
    }

    /** `(c "Int" "42")` as a `universe.Constant`. */
    static Object constant(Sexp s) throws Exception {
        String kind = s.items.get(1).text();
        String text = s.items.get(2).text();
        Object v;
        switch (kind) {
            case "Unit": v = boxedUnit(); break;
            case "Boolean": v = Boolean.valueOf(text); break;
            case "Char": v = Character.valueOf(text.charAt(0)); break;
            case "Int": v = Integer.valueOf(text); break;
            case "Long": v = Long.valueOf(text); break;
            case "Float": v = Float.valueOf(text); break;
            case "Double": v = Double.valueOf(text); break;
            case "String": v = text; break;
            case "Null": v = null; break;
            default: throw new IllegalArgumentException("unknown constant kind " + kind);
        }
        return call(companion("Constant"), "apply", 1, v);
    }

    /** `(ty "java.lang.String")` as a `universe.WeakTypeTag`. */
    static Object buildTag(Sexp s) throws Exception {
        return tagFor(s.items.get(1).text());
    }

    /** `WeakTypeTag` for the class `name`, in the runtime universe. */
    static Object tagFor(String name) throws Exception {
        Object cls = call(mirror, "staticClass", 1, name);
        Object tpe = call(call(cls, "asType", 0), "toType", 0);
        Class<?> creatorCls =
            Class.forName("scala.reflect.internal.StdCreators$FixedMirrorTypeCreator", true, macroCl);
        Object creator = ctor(creatorCls, 3).newInstance(universe, mirror, tpe);
        return call(companion("WeakTypeTag"), "apply", 2, mirror, creator);
    }

    /** `universe.Expr(mirror, FixedMirrorTreeCreator(mirror, tree))(tag)`. */
    static Object mkExpr(Object tree, Object tag) throws Exception {
        Class<?> creatorCls =
            Class.forName("scala.reflect.internal.StdCreators$FixedMirrorTreeCreator", true, macroCl);
        Object creator = ctor(creatorCls, 3).newInstance(universe, mirror, tree);
        return call(companion("Expr"), "apply", 3, mirror, creator, tag);
    }

    // ------------------------------------------------------- serialising back

    /**
     * One value of a reflect tree, written back generically.
     *
     * The engine deliberately does not know which node kinds scala-rs can
     * rebuild: it writes `productPrefix` and the product elements, and the
     * Rust side rejects, by name, anything it cannot turn into a tree of its
     * own. That keeps "unknown node" a diagnostic instead of a wrong tree.
     */
    static void ser(Object o, StringBuilder sb) throws Exception {
        if (o == null) {
            sb.append("(o \"null\")");
            return;
        }
        if (isA(o, "scala.reflect.api.Trees$TreeApi")) {
            serTree(o, sb);
            return;
        }
        if (isA(o, "scala.reflect.api.Names$NameApi")) {
            boolean term = (Boolean) call(o, "isTermName", 0);
            sb.append("(n ").append(term ? "term" : "type").append(' ')
              .append(Sexp.quote(o.toString())).append(')');
            return;
        }
        if (isA(o, "scala.reflect.api.Constants$ConstantApi")) {
            serConstant(o, sb);
            return;
        }
        if (isA(o, "scala.reflect.api.Trees$ModifiersApi")) {
            serMods(o, sb);
            return;
        }
        if (isA(o, "scala.collection.immutable.List")) {
            sb.append("(l");
            Object it = call(o, "iterator", 0);
            while ((Boolean) call(it, "hasNext", 0)) {
                sb.append(' ');
                ser(call(it, "next", 0), sb);
            }
            sb.append(')');
            return;
        }
        sb.append("(o ").append(Sexp.quote(String.valueOf(o))).append(')');
    }

    static void serTree(Object t, StringBuilder sb) throws Exception {
        Object empty = call(universe, "EmptyTree", 0);
        if (t == empty) {
            sb.append("(t \"EmptyTree\" (s0))");
            return;
        }
        String prefix;
        try {
            prefix = String.valueOf(call(t, "productPrefix", 0));
        } catch (Throwable e) {
            prefix = t.getClass().getSimpleName();
        }
        sb.append("(t ").append(Sexp.quote(prefix)).append(' ');
        serSym(t, sb);
        if ("TypeTree".equals(prefix)) {
            // A `TypeTree` carries its type, not children: writing the type is
            // the only way the call site can rebuild it.
            sb.append(' ');
            serType(call(t, "tpe", 0), sb);
            sb.append(')');
            return;
        }
        int arity = (Integer) call(t, "productArity", 0);
        for (int i = 0; i < arity; i++) {
            sb.append(' ');
            ser(call(t, "productElement", 1, Integer.valueOf(i)), sb);
        }
        sb.append(')');
    }

    /** The tree's symbol, when it is one a name can find again. */
    static void serSym(Object t, StringBuilder sb) {
        try {
            Object sym = call(t, "symbol", 0);
            if (sym == null || (Boolean) call(sym, "isEmpty", 0)) {
                sb.append("(s0)");
                return;
            }
            // Only a *static* symbol survives the trip: scala-rs resolves it
            // by full name, and a local or a parameter has no such name.
            Object isStatic = call(sym, "isStatic", 0);
            if (!Boolean.TRUE.equals(isStatic)) {
                sb.append("(s0)");
                return;
            }
            sb.append("(s ").append(Sexp.quote(String.valueOf(call(sym, "fullName", 0))))
              .append(')');
        } catch (Throwable e) {
            sb.append("(s0)");
        }
    }

    static void serType(Object tpe, StringBuilder sb) throws Exception {
        if (tpe == null) {
            sb.append("(ty \"\")");
            return;
        }
        Object sym = call(tpe, "typeSymbol", 0);
        String name = String.valueOf(call(sym, "fullName", 0));
        sb.append("(ty ").append(Sexp.quote(name));
        Object args = call(tpe, "typeArgs", 0);
        Object it = call(args, "iterator", 0);
        while ((Boolean) call(it, "hasNext", 0)) {
            sb.append(' ');
            serType(call(it, "next", 0), sb);
        }
        sb.append(')');
    }

    static void serConstant(Object c, StringBuilder sb) throws Exception {
        Object v = call(c, "value", 0);
        String kind;
        String text;
        if (v == null) {
            kind = "Null";
            text = "null";
        } else if (v instanceof Boolean) {
            kind = "Boolean";
            text = v.toString();
        } else if (v instanceof Character) {
            kind = "Char";
            text = v.toString();
        } else if (v instanceof Byte) {
            kind = "Byte";
            text = v.toString();
        } else if (v instanceof Short) {
            kind = "Short";
            text = v.toString();
        } else if (v instanceof Integer) {
            kind = "Int";
            text = v.toString();
        } else if (v instanceof Long) {
            kind = "Long";
            text = v.toString();
        } else if (v instanceof Float) {
            kind = "Float";
            text = v.toString();
        } else if (v instanceof Double) {
            kind = "Double";
            text = v.toString();
        } else if (v instanceof String) {
            kind = "String";
            text = (String) v;
        } else if (isA(v, "scala.runtime.BoxedUnit")) {
            kind = "Unit";
            text = "()";
        } else if (isA(v, "scala.reflect.api.Types$TypeApi")) {
            kind = "Type";
            text = String.valueOf(v);
        } else {
            kind = "Other";
            text = String.valueOf(v);
        }
        sb.append("(c ").append(Sexp.quote(kind)).append(' ')
          .append(Sexp.quote(text)).append(')');
    }

    /**
     * `Modifiers`, as the *names* of the flags that are set.
     *
     * The flag values are read off `universe.Flag` reflectively rather than
     * hard-coded: nsc's bit layout is an internal detail, several bits carry
     * two names (`BYNAMEPARAM` is `COVARIANT`, `DEFAULTPARAM` is `TRAIT`), and
     * a number on the wire would make scala-rs guess. Every name whose bit is
     * set is written, and whatever bits are left over travel as a hex number
     * so the Rust side can refuse a modifier it has no name for rather than
     * dropping it.
     *
     * `privateWithin` and the annotations travel too, for the same reason: a
     * `ValDef` scala-rs rebuilds without them would be a different definition.
     */
    static void serMods(Object mods, StringBuilder sb) throws Exception {
        long flags = ((Number) call(mods, "flags", 0)).longValue();
        sb.append("(mods (f");
        long known = 0;
        Object flagValues = call(universe, "Flag", 0);
        for (Method m : flagValues.getClass().getMethods()) {
            if (m.getParameterCount() != 0 || m.getReturnType() != long.class) {
                continue;
            }
            String n = m.getName();
            if (!n.equals(n.toUpperCase()) || n.isEmpty()) {
                continue;
            }
            m.setAccessible(true);
            long v = ((Number) m.invoke(flagValues)).longValue();
            if (v != 0 && (flags & v) == v) {
                known |= v;
                sb.append(' ').append(Sexp.quote(n));
            }
        }
        sb.append(") (rest ").append(Sexp.quote(Long.toHexString(flags & ~known))).append(") ");
        Object pw = call(mods, "privateWithin", 0);
        sb.append(Sexp.quote(pw == null ? "" : String.valueOf(pw))).append(' ');
        ser(call(mods, "annotations", 0), sb);
        sb.append(')');
    }

    // -------------------------------------------------------------- Context

    /** `c.abort` -- the macro asked for a compile error at a position. */
    static final class Abort extends RuntimeException {
        private static final long serialVersionUID = 1L;

        Abort(String msg) {
            super(msg);
        }
    }

    static final class Ctx implements InvocationHandler {
        /** The receiver of this macro application, or null. */
        Object prefixTree;
        /** Why there is no prefix tree, when there is none. */
        String prefixWhy;
        /** `c.macroApplication`: the whole call as written, or null. */
        Object appTree;
        /** Why there is no application tree, when there is none. */
        String appWhy;
        /** Built once: `prefix` is a `val` in nsc and is read more than once. */
        Object prefix;

        public Object invoke(Object proxy, Method m, Object[] a) throws Throwable {
            String n = m.getName();
            int arity = m.getParameterCount();
            if (n.equals("prefix") && arity == 0) {
                if (prefixTree == null) {
                    throw new UnsupportedOperationException(
                        "scala-rs macro engine: c.prefix is not available here -- " + prefixWhy);
                }
                if (prefix == null) {
                    // nsc: `Expr[Nothing](prefixTree)(TypeTag.Nothing)`. The
                    // prefix carries no type of its own -- `PrefixType` is an
                    // abstract member of the blackbox `Context` -- so
                    // `c.prefix.staticType` is `Nothing` there too.
                    prefix = mkExpr(prefixTree, call(companion("TypeTag"), "Nothing", 0));
                }
                return prefix;
            }
            if (n.equals("macroApplication") && arity == 0) {
                if (appTree == null) {
                    throw new UnsupportedOperationException(
                        "scala-rs macro engine: c.macroApplication is not available here -- "
                            + appWhy);
                }
                return appTree;
            }
            if (n.equals("compilerSettings") && arity == 0) {
                return list(new ArrayList<Object>(compilerSettings));
            }
            switch (n) {
                case "universe":
                    return universe;
                case "mirror":
                    return mirror;
                case "toString":
                    return "scala-rs macro Context";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == a[0];
                default:
                    break;
            }
            // The `Aliases` vals: hand back the universe's own companions.
            if (arity == 0 && (n.equals("Expr") || n.equals("WeakTypeTag")
                    || n.equals("TypeTag") || n.equals("TypeName") || n.equals("TermName"))) {
                return call(universe, n, 0);
            }
            if (n.equals("Expr") && arity == 2) {
                return mkExpr(a[0], a[1]);
            }
            if (n.startsWith("scala$reflect$macros$") && n.contains("_setter_")) {
                return null;
            }
            if (n.equals("freshName")) {
                fresh++;
                if (arity == 0) {
                    return "fresh$macro$" + fresh;
                }
                if (a[0] instanceof String) {
                    return a[0] + "$macro$" + fresh;
                }
                // freshName(name: Name): Name
                boolean term = (Boolean) call(a[0], "isTermName", 0);
                String s = a[0] + "$macro$" + fresh;
                return term ? termName(s) : typeName(s);
            }
            if (n.equals("abort")) {
                throw new Abort(String.valueOf(a[a.length - 1]));
            }
            if (m.isDefault()) {
                return InvocationHandler.invokeDefault(proxy, m, a);
            }
            throw new UnsupportedOperationException(
                "scala-rs macro engine: Context." + n + " is not implemented");
        }
    }

    // ------------------------------------------------------------- plumbing

    static Object companion(String name) throws Exception {
        return call(universe, name, 0);
    }

    static Object termName(String s) throws Exception {
        return call(companion("TermName"), "apply", 1, s);
    }

    static Object typeName(String s) throws Exception {
        return call(companion("TypeName"), "apply", 1, s);
    }

    static Object boxedUnit() throws Exception {
        return Class.forName("scala.runtime.BoxedUnit", true, macroCl)
            .getField("UNIT").get(null);
    }

    /** `List(xs)` in the immutable Scala list, built from `Nil` and `::`. */
    static Object list(List<Object> xs) throws Exception {
        Object acc = Class.forName("scala.collection.immutable.Nil$", true, macroCl)
            .getField("MODULE$").get(null);
        Class<?> cons = Class.forName("scala.collection.immutable.$colon$colon", true, macroCl);
        Constructor<?> c = ctor(cons, 2);
        for (int i = xs.size() - 1; i >= 0; i--) {
            acc = c.newInstance(xs.get(i), acc);
        }
        return acc;
    }

    static boolean isA(Object o, String cls) {
        try {
            return Class.forName(cls, true, macroCl).isInstance(o);
        } catch (Throwable t) {
            return false;
        }
    }

    /**
     * Invoke `name` on `recv`.
     *
     * Arity alone is not enough to pick the method: the reflect API overloads
     * several extractors on it (`Ident.apply(Name)` and `Ident.apply(Symbol)`,
     * `This`, `Bind`, `New`), and taking whichever `getMethods` returns first
     * threw `IllegalArgumentException` for half of them. Prefer an overload
     * whose parameter types actually accept these arguments.
     */
    static Object call(Object recv, String name, int arity, Object... args) throws Exception {
        Method fallback = null;
        for (Method m : recv.getClass().getMethods()) {
            if (!m.getName().equals(name) || m.getParameterCount() != arity) {
                continue;
            }
            if (fallback == null) {
                fallback = m;
            }
            if (accepts(m.getParameterTypes(), args)) {
                m.setAccessible(true);
                return m.invoke(recv, args);
            }
        }
        if (fallback == null) {
            throw new IllegalStateException(
                "no " + name + "/" + arity + " on " + recv.getClass().getName());
        }
        fallback.setAccessible(true);
        return fallback.invoke(recv, args);
    }

    static boolean accepts(Class<?>[] want, Object[] args) {
        for (int i = 0; i < want.length; i++) {
            Object a = i < args.length ? args[i] : null;
            if (a == null) {
                continue;
            }
            if (want[i].isPrimitive() || want[i].isInstance(a)) {
                continue;
            }
            return false;
        }
        return true;
    }

    static Method find(Class<?> c, String name, int arity) {
        for (Method m : c.getMethods()) {
            if (m.getName().equals(name) && m.getParameterCount() == arity) {
                m.setAccessible(true);
                return m;
            }
        }
        throw new IllegalStateException("no " + name + "/" + arity + " on " + c.getName());
    }

    static Constructor<?> ctor(Class<?> c, int arity) {
        for (Constructor<?> k : c.getConstructors()) {
            if (k.getParameterCount() == arity) {
                return k;
            }
        }
        throw new IllegalStateException("no " + arity + "-arg constructor on " + c.getName());
    }

    static String describe(Throwable t) {
        while (t instanceof InvocationTargetException && t.getCause() != null) {
            t = t.getCause();
        }
        String m = t.getMessage();
        return t.getClass().getName() + (m == null ? "" : ": " + m);
    }

    static String err(String msg) {
        return "(err " + Sexp.quote(msg) + ")";
    }

    // ------------------------------------------------------------------ sexp

    /** The wire format: atoms, quoted strings and lists. */
    static final class Sexp {
        String atom;
        List<Sexp> items;

        boolean isList() {
            return items != null;
        }

        String text() {
            return atom;
        }

        /** The list whose head atom is `name`. */
        Sexp field(String name) {
            for (Sexp s : items) {
                if (s.isList() && !s.items.isEmpty() && name.equals(s.items.get(0).atom)) {
                    return s;
                }
            }
            throw new IllegalArgumentException("no field " + name);
        }

        public String toString() {
            if (!isList()) {
                return atom;
            }
            StringBuilder sb = new StringBuilder("(");
            for (int i = 0; i < items.size(); i++) {
                if (i > 0) {
                    sb.append(' ');
                }
                sb.append(items.get(i));
            }
            return sb.append(')').toString();
        }

        static Sexp parse(String s) {
            int[] p = {0};
            Sexp v = parse(s, p);
            return v;
        }

        static Sexp parse(String s, int[] p) {
            while (p[0] < s.length() && s.charAt(p[0]) == ' ') {
                p[0]++;
            }
            char c = s.charAt(p[0]);
            Sexp v = new Sexp();
            if (c == '(') {
                p[0]++;
                v.items = new ArrayList<>();
                while (true) {
                    while (p[0] < s.length() && s.charAt(p[0]) == ' ') {
                        p[0]++;
                    }
                    if (p[0] >= s.length()) {
                        break;
                    }
                    if (s.charAt(p[0]) == ')') {
                        p[0]++;
                        break;
                    }
                    v.items.add(parse(s, p));
                }
                return v;
            }
            if (c == '"') {
                p[0]++;
                StringBuilder sb = new StringBuilder();
                while (p[0] < s.length() && s.charAt(p[0]) != '"') {
                    char ch = s.charAt(p[0]++);
                    if (ch == '\\') {
                        char e = s.charAt(p[0]++);
                        sb.append(e == 'n' ? '\n' : e == 't' ? '\t' : e);
                    } else {
                        sb.append(ch);
                    }
                }
                p[0]++;
                v.atom = sb.toString();
                return v;
            }
            StringBuilder sb = new StringBuilder();
            while (p[0] < s.length() && " ()".indexOf(s.charAt(p[0])) < 0) {
                sb.append(s.charAt(p[0]++));
            }
            v.atom = sb.toString();
            return v;
        }

        static String quote(String s) {
            StringBuilder sb = new StringBuilder("\"");
            if (s == null) {
                s = "";
            }
            for (int i = 0; i < s.length(); i++) {
                char c = s.charAt(i);
                if (c == '"' || c == '\\') {
                    sb.append('\\').append(c);
                } else if (c == '\n') {
                    sb.append("\\n");
                } else if (c == '\t') {
                    sb.append("\\t");
                } else if (c == '\r') {
                    sb.append("\\r");
                } else {
                    sb.append(c);
                }
            }
            return sb.append('"').toString();
        }
    }
}
