import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.lang.invoke.MethodHandles;
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
    static final java.util.Map<Long, Object> sourceSymbols = new java.util.HashMap<>();
    static final java.util.IdentityHashMap<Object, Long> sourceSymbolIds = new java.util.IdentityHashMap<>();
    static Class<?> lazyInfoClass;
    static final java.util.Map<String, Class<?>> sourceSymbolClasses = new java.util.HashMap<>();
    static final java.util.Set<Object> mutableSourceSymbols = java.util.Collections.newSetFromMap(new java.util.IdentityHashMap<>());
    static final class MacroOutput extends java.io.OutputStream {
        final String channel;
        final java.io.ByteArrayOutputStream pending = new java.io.ByteArrayOutputStream();
        MacroOutput(String channel) { this.channel = channel; }
        public synchronized void write(int value) {
            pending.write(value);
            if (value == '\n') flush();
        }
        public synchronized void write(byte[] bytes, int offset, int length) {
            for (int i = offset; i < offset + length; i++) write(bytes[i]);
        }
        public synchronized void flush() {
            if (pending.size() == 0) return;
            String text = new String(pending.toByteArray(), StandardCharsets.UTF_8);
            pending.reset();
            out.println("(log " + channel + " " + Sexp.quote(text) + ")");
        }
    }

    static Object universe;
    static Object mirror;
    static ClassLoader macroCl;
    /**
     * The pipe, as fields, because expansion is a *conversation* rather than
     * one line in and one line out: an implementation may stop mid-flight and
     * ask scala-rs a question ({@link #query}), which is written and read on
     * the same two streams the request came in on.
     */
    static BufferedReader in;
    static PrintStream out;
    /**
     * Set when scala-rs answered a question with "scala-rs cannot answer
     * this". It is raised as an {@link Gap}, which is an `Error` so that an
     * implementation's own `catch (ex: Exception)` does not swallow it -- and
     * remembered here as well, because several implementations catch
     * `Throwable`. A gap that the implementation swallowed still ends the
     * expansion with the reason, rather than letting it build a tree from an
     * answer it did not get.
     */
    static String pendingGap;
    /**
     * The message of a `TypecheckException` this engine raised for a failed
     * `c.typecheck`, cleared when the implementation returns.
     *
     * It is here because the exception **cannot reach the implementation**.
     * The `Context` is a `java.lang.reflect.Proxy`, and a proxy wraps any
     * checked exception the interface method does not declare in an
     * `UndeclaredThrowableException`; `TypecheckException extends Exception`
     * and `Typers.typecheck` declares nothing. So an implementation that
     * writes `catch { case c.TypecheckException(_, msg) => ... }` -- which is
     * how the failure is meant to be handled -- does not match, and one that
     * catches `Throwable` sees the wrapper instead. Either way its answer is
     * built on something it was not told, so the expansion ends with this
     * reason rather than with that answer. Closing the gap needs a generated
     * `Context` class instead of a proxy.
     */
    static String pendingTypecheckFailure;
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
        out = new PrintStream(System.out, true, "UTF-8");
        in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        // Keep the original stream for protocol packets; macro println/Console
        // output is payload, never another expansion reply.
        System.setOut(new PrintStream(new MacroOutput("stdout"), true, "UTF-8"));
        System.setErr(new PrintStream(new MacroOutput("stderr"), true, "UTF-8"));
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

        origTrees.clear();
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
            Sexp position = req.field("position");
            if (position.items.size() == 4) {
                Class<?> virtual = Class.forName("scala.reflect.io.VirtualFile", true, macroCl);
                String path = position.items.get(2).text();
                Object file = virtual.getConstructor(String.class, String.class)
                    .newInstance(new java.io.File(path).getName(), path);
                Class<?> abstractFile = Class.forName("scala.reflect.io.AbstractFile", true, macroCl);
                Class<?> sourceClass = Class.forName("scala.reflect.internal.util.SourceFile", true, macroCl);
                Object source = Class.forName("scala.reflect.internal.util.BatchSourceFile", true, macroCl)
                    .getConstructor(abstractFile, char[].class)
                    .newInstance(file, position.items.get(1).text().toCharArray());
                Object pos = Class.forName("scala.reflect.internal.util.OffsetPosition", true, macroCl)
                    .getConstructor(sourceClass, int.class)
                    .newInstance(source, Integer.parseInt(position.items.get(3).text()));
                call(handler.appTree, "setPos", 1, pos);
            }
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
                if ("repeat".equals(a.items.get(0).atom)) {
                    List<Object> values = new ArrayList<>();
                    for (Sexp value : a.items.subList(1, a.items.size())) values.add(buildArgument(value));
                    argv.add(list(values));
                } else {
                    argv.add(buildArgument(a));
                }
            }
        }
        for (Sexp t : tags.items.subList(1, tags.items.size())) {
            argv.add(buildTag(t));
        }
        if (argv.size() != impl.getParameterCount()) {
            return err("macro implementation " + className + "." + methodName + " takes "
                + impl.getParameterCount() + " arguments, the call site supplies " + argv.size());
        }

        pendingGap = null;
        pendingTypecheckFailure = null;
        Object result;
        try {
            result = impl.invoke(receiver, argv.toArray());
        } catch (InvocationTargetException e) {
            Throwable cause = e.getCause();
            if (pendingGap != null) {
                return err(pendingGap);
            }
            if (pendingTypecheckFailure != null) {
                return err("c.typecheck rejected the tree the implementation gave it: "
                    + pendingTypecheckFailure);
            }
            if (cause instanceof Abort) {
                return "(abort " + Sexp.quote(cause.getMessage()) + ")";
            }
            return err("the macro implementation threw " + describe(cause));
        }
        // A gap the implementation caught and carried on from. Its answer is
        // built on something scala-rs never told it, so the reason is
        // reported instead of the tree.
        if (pendingGap != null) {
            return err(pendingGap);
        }
        if (pendingTypecheckFailure != null) {
            return err("c.typecheck rejected the tree the implementation gave it ("
                + pendingTypecheckFailure + "), and the implementation caught the failure. "
                + "scala-rs cannot hand a TypecheckException to an implementation -- the "
                + "Context is a java.lang.reflect.Proxy, which wraps it -- so what it did "
                + "next was decided on something it was not told");
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

    // ------------------------------------------------------- reverse RPC

    /**
     * A question scala-rs could not answer.
     *
     * An `Error` rather than an `Exception` on purpose: an implementation that
     * writes `try c.typecheck(t) catch { case ex: Exception => ... }` -- and
     * several in the wild do -- must not be able to turn "scala-rs has no
     * answer" into a tree of its own devising. {@link #pendingGap} catches the
     * remaining `catch (ex: Throwable)` case.
     */
    static final class Gap extends Error {
        private static final long serialVersionUID = 1L;

        Gap(String msg) {
            super(msg);
        }
    }

    static Gap gap(String why) {
        pendingGap = why;
        return new Gap(why);
    }

    /**
     * Ask scala-rs a question in the middle of an expansion.
     *
     * This is the reverse direction of the bridge (`docs/macros.md` §7.18).
     * The engine writes `(q ...)` on the same stdout the reply goes to, and
     * scala-rs -- which is sitting in its read loop waiting for that reply --
     * recognises the `q`, answers on stdin, and goes back to waiting. So the
     * two processes take turns and the pipe stays in step: exactly one line is
     * written and exactly one is read.
     *
     * The answer is `(a ...)`, or `(no "reason")` for a question scala-rs
     * cannot answer, which is raised as a {@link Gap} and becomes the call
     * site's diagnostic.
     */
    static Sexp query(String q) throws Exception {
        out.println(q);
        String line = in.readLine();
        if (line == null) {
            throw new Gap("scala-rs closed the pipe while the macro was asking it a question");
        }
        Sexp ans = Sexp.parse(line);
        if (ans.isList() && !ans.items.isEmpty() && "no".equals(ans.items.get(0).atom)) {
            throw gap(ans.items.get(1).text());
        }
        return ans;
    }

    /**
     * `c.typecheck(tree, mode, pt, silent, ...)`.
     *
     * nsc typechecks the tree in the macro call site's own context. scala-rs
     * is where that context lives, so the tree goes back over the wire, is
     * typed there for real, and comes back as the tree the typer made of it
     * with its type set on it.
     *
     * The arguments this bridge cannot honour are refused by name rather than
     * ignored. A `pt` other than `WildcardType` asks a different question from
     * the one that is sent; `withImplicitViewsDisabled` and
     * `withMacrosDisabled` ask for a typer mode scala-rs has no switch for.
     * Ignoring any of the three would answer a question nobody asked.
     */
    static Object typecheck(Object tree, Object mode, Object pt, boolean silent,
                            boolean noViews, boolean noMacros) throws Exception {
        if (pt != null && pt != call(universe, "WildcardType", 0)) {
            throw gap("c.typecheck was given the expected type `" + pt
                + "`; scala-rs types the tree with no expectation and cannot honour one yet");
        }
        if (noViews) {
            throw gap("c.typecheck was asked to disable implicit views, which scala-rs's "
                + "typer has no switch for");
        }
        if (noMacros) {
            throw gap("c.typecheck was asked to disable macro expansion, which scala-rs's "
                + "typer has no switch for");
        }
        String modeName = String.valueOf(mode);
        if (!"TERM".equals(modeName) && !"TYPE".equals(modeName)
                && !"PATTERN".equals(modeName)) {
            throw gap("c.typecheck was given the mode `" + modeName
                + "`, which did not come from c.TERMmode, c.TYPEmode or c.PATTERNmode");
        }
        StringBuilder sb = new StringBuilder("(q typecheck ");
        ser(tree, sb);
        sb.append(' ').append(Sexp.quote(modeName)).append(' ')
          .append(silent ? "1" : "0").append(')');
        Sexp ans = query(sb.toString());
        String verdict = ans.items.get(1).atom;
        if ("fail".equals(verdict)) {
            String msg = ans.items.get(2).text();
            if (silent) {
                // nsc: a silent typecheck that fails is `EmptyTree`.
                return call(universe, "EmptyTree", 0);
            }
            pendingTypecheckFailure = msg;
            sneakyThrow(newTypecheckException(msg));
        }
        Object tpe = typeFor(ans.items.get(2));
        Object built = buildTree(ans.items.get(3));
        Object support = call(call(universe, "internal", 0), "reificationSupport", 0);
        // Attachments belong to the original JVM tree. A typer adaptation may
        // change its shape (Ident to Select), but must retain those attachments.
        call(built, "setAttachments", 1, call(tree, "attachments", 0));
        return call(support, "setType", 2, built, tpe);
    }

    /** `scala.reflect.macros.TypecheckException(NoPosition, msg)`. */
    static Throwable newTypecheckException(String msg) throws Exception {
        Class<?> cls = Class.forName("scala.reflect.macros.TypecheckException", true, macroCl);
        Object pos = call(universe, "NoPosition", 0);
        return (Throwable) ctor(cls, 2).newInstance(pos, msg);
    }

    @SuppressWarnings("unchecked")
    static <T extends Throwable> void sneakyThrow(Throwable t) throws T {
        throw (T) t;
    }

    // ------------------------------------------------------- building trees

    /**
     * The receiver and arguments of the macro application, by the index
     * scala-rs sent each under (`(orig K <tree>)`). A tree the implementation
     * returns unchanged -- the same object -- goes back as `(t "Orig" (s0) K
     * <tree>)`, and scala-rs puts its own *typed* tree in its place, the way
     * nsc splices a typed tree without typing it again.
     */
    static final java.util.IdentityHashMap<Object, Long> origTrees = new java.util.IdentityHashMap<>();

    /** A tree the request describes, built in the runtime universe. */
    static Object buildTree(Sexp s) throws Exception {
        if (s.isList() && s.items.size() == 3 && "orig".equals(s.items.get(0).atom)) {
            Object tree = buildTree(s.items.get(2));
            origTrees.put(tree, Long.parseLong(s.items.get(1).text()));
            return tree;
        }
        Object symbol = null;
        if (s.isList() && s.items.size() >= 3 && "t".equals(s.items.get(0).atom)) {
            String kind = s.items.get(1).text();
            Sexp meta = s.items.get(2);
            if (meta.isList() && !meta.items.isEmpty()) {
                if ("fn".equals(meta.items.get(0).atom)) {
                    Sexp answer = query("(q functionSymbol " + meta.items.get(1).text() + ")");
                    symbol = sourceSymbol(Long.parseLong(answer.items.get(2).text()));
                } else if ("sr".equals(meta.items.get(0).atom)) {
                    long id = Long.parseLong(meta.items.get(1).text());
                    if ("ValDef".equals(kind) || "DefDef".equals(kind) || "ClassDef".equals(kind)
                            || "Function".equals(kind)) symbol = sourceSymbol(id);
                    else symbol = sourceSymbols.get(id);
                }
            }
        }
        Object tree = buildTreeBody(s);
        if (symbol != null && Boolean.TRUE.equals(call(tree, "hasSymbolField", 0))) {
            call(tree, "setSymbol", 1, symbol);
        }
        return tree;
    }

    static Object buildTreeBody(Sexp s) throws Exception {
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
                return call(companion("Ident"), "apply", 1, buildName(kids.get(0)));
            case "This":
                return call(companion("This"), "apply", 1, typeName(nameOf(kids.get(0))));
            case "Select":
                return call(companion("Select"), "apply", 2,
                    buildTree(kids.get(0)), buildName(kids.get(1)));
            case "Apply":
            case "TypeApply":
            case "AppliedTypeTree": {
                Object fun = buildTree(kids.get(0));
                List<Object> as = new ArrayList<>();
                for (Sexp k : kids.get(1).items.subList(1, kids.get(1).items.size())) {
                    as.add(buildTree(k));
                }
                return call(companion(kind), "apply", 2, fun, list(as));
            }
            case "Block": {
                List<Object> stats = new ArrayList<>();
                for (Sexp k : kids.get(0).items.subList(1, kids.get(0).items.size())) {
                    stats.add(buildTree(k));
                }
                return call(companion("Block"), "apply", 2, list(stats), buildTree(kids.get(1)));
            }
            case "If":
                return call(companion("If"), "apply", 3, buildTree(kids.get(0)),
                    buildTree(kids.get(1)), buildTree(kids.get(2)));
            case "TypeTree": {
                Object tree = call(companion("TypeTree"), "apply", 0);
                if (!kids.isEmpty() && !kids.get(0).items.get(1).text().isEmpty()) {
                    Object support = call(call(universe, "internal", 0), "reificationSupport", 0);
                    call(support, "setType", 2, tree, typeFor(kids.get(0)));
                }
                return tree;
            }
            case "Super":
                return call(companion(kind), "apply", 2, buildTree(kids.get(0)), buildName(kids.get(1)));
            case "New":
                return call(companion(kind), "apply", 1, buildTree(kids.get(0)));
            case "Typed":
            case "Assign":
            case "Annotated":
                return call(companion(kind), "apply", 2,
                    buildTree(kids.get(0)), buildTree(kids.get(1)));
            case "Function":
                return call(companion(kind), "apply", 2,
                    buildTrees(kids.get(0)), buildTree(kids.get(1)));
            case "ValDef":
                return call(companion(kind), "apply", 4, buildMods(kids.get(0)),
                    buildName(kids.get(1)), buildTree(kids.get(2)), buildTree(kids.get(3)));
            case "DefDef": {
                List<Object> clauses = new ArrayList<>();
                for (Sexp clause : kids.get(3).items.subList(1, kids.get(3).items.size())) {
                    clauses.add(buildTrees(clause));
                }
                return call(companion(kind), "apply", 6, buildMods(kids.get(0)),
                    buildName(kids.get(1)), buildTrees(kids.get(2)), list(clauses),
                    buildTree(kids.get(4)), buildTree(kids.get(5)));
            }
            case "ClassDef":
                return call(companion(kind), "apply", 4, buildMods(kids.get(0)),
                    buildName(kids.get(1)), buildTrees(kids.get(2)), buildTree(kids.get(3)));
            case "Template":
                return call(companion(kind), "apply", 3, buildTrees(kids.get(0)),
                    buildTree(kids.get(1)), buildTrees(kids.get(2)));
            default:
                throw new IllegalArgumentException(
                    "scala-rs cannot hand a " + kind + " to a macro implementation");
        }
    }

    static Object buildArgument(Sexp a) throws Exception {
        boolean asExpr = "expr".equals(a.items.get(1).atom);
        Object tree = buildTree(a.items.get(2));
        // Expr's weak tag remains Nothing, but its already-typed argument
        // tree carries the actual source type, including literal constants.
        call(tree, "setType", 1, typeFor(a.items.get(4)));
        return asExpr ? mkExpr(tree, buildTag(a.items.get(3))) : tree;
    }

    static Object buildName(Sexp s) throws Exception {
        return "type".equals(s.items.get(1).atom) ? typeName(nameOf(s)) : termName(nameOf(s));
    }

    static Object buildTrees(Sexp s) throws Exception {
        List<Object> trees = new ArrayList<>();
        for (Sexp t : s.items.subList(1, s.items.size())) trees.add(buildTree(t));
        return list(trees);
    }

    static Object buildMods(Sexp s) throws Exception {
        long flags = flagsOf(s.items.get(1));
        flags |= Long.parseUnsignedLong(s.items.get(2).items.get(1).text(), 16);
        return call(companion("Modifiers"), "apply", 3, Long.valueOf(flags),
            typeName(s.items.get(3).text()), buildTrees(s.items.get(4)));
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

    /**
     * A type descriptor as a `universe.WeakTypeTag` ({@link #typeFor}).
     */
    static Object buildTag(Sexp s) throws Exception {
        return tagOf(typeFor(s));
    }

    /**
     * The `universe.Type` a type descriptor names.
     *
     * `(ty "a.b.C" <arg>…)` is a class the mirror finds on the macro
     * classpath, applied to its type arguments; `(src <id>)` is one the
     * calling run is compiling, which has no class file for the mirror to
     * find, built by {@link #sourceSymbol} with its info asked for only when
     * forced; `(annot "A" <type>)` is `<type> @A`; `(cst (c "Int" "1"))` is
     * the constant type nsc gives a literal, which `c.typecheck(q"1").tpe`
     * has to be if it is to be the type nsc reports.
     */
    static Object typeFor(Sexp s) throws Exception {
        String head = s.items.get(0).atom;
        if ("cst".equals(head)) {
            return call(call(universe, "internal", 0), "constantType", 1,
                constant(s.items.get(1)));
        }
        String name = s.items.get(1).text();
        if ("src".equals(head)) {
            Object sym = sourceSymbol(Long.parseLong(name));
            return ownedTypeRef(call(sym, "owner", 0), sym);
        }
        if ("annot".equals(head)) {
            // `T @A` for an annotation class `A` taking no arguments -- the
            // `@uncheckedVariance` nsc puts on a default getter's result.
            Object under = typeFor(s.items.get(2));
            Object annTpe = call(call(call(mirror, "staticClass", 1, name), "asType", 0), "toType", 0);
            Object noJavaArgs = call(Class.forName("scala.collection.immutable.ListMap$", true, macroCl)
                .getField("MODULE$").get(null), "empty", 0);
            Object ann = call(companion("Annotation"), "apply", 3, annTpe,
                list(new ArrayList<>()), noJavaArgs);
            List<Object> anns = new ArrayList<>();
            anns.add(ann);
            return call(call(universe, "internal", 0), "annotatedType", 2, list(anns), under);
        }
        Object cls = call(mirror, "staticClass", 1, name);
        if (s.items.size() <= 2) {
            return call(call(cls, "asType", 0), "toType", 0);
        }
        List<Object> args = new ArrayList<>();
        for (Sexp a : s.items.subList(2, s.items.size())) {
            args.add(typeFor(a));
        }
        return call(universe, "appliedType", 2, cls, list(args));
    }

    /** `WeakTypeTag` for a type already built in the runtime universe. */
    static Object tagOf(Object tpe) throws Exception {
        Class<?> creatorCls =
            Class.forName("scala.reflect.internal.StdCreators$FixedMirrorTypeCreator", true, macroCl);
        Object creator = ctor(creatorCls, 3).newInstance(universe, mirror, tpe);
        return call(companion("WeakTypeTag"), "apply", 2, mirror, creator);
    }

    /**
     * Types standing for classes the *calling* compilation run is defining,
     * keyed by their full names, so the same class is always the same symbol
     * and an expansion that mentions it twice mentions one type.
     */
    static final java.util.HashMap<String, Object> synthetic = new java.util.HashMap<>();

    /**
     * A `(f "NAME" ...)` list as nsc's flag bits.
     *
     * The names are looked up on `universe.Flag` rather than hard-coded, for
     * the reason {@link #serMods} gives in the other direction: the bit layout
     * is an internal detail. A name that is not there is an error, never a
     * silently dropped flag -- a member described without `DEFERRED` is a
     * different member.
     */
    static long flagsOf(Sexp f) throws Exception {
        long flags = 0;
        for (Sexp n : f.items.subList(1, f.items.size())) {
            String name = n.text();
            // Not flags: markers that say which *kind* of symbol to build.
            // `METHOD` and `CONSTRUCTOR` are internal bits `newMethodSymbol`
            // sets on its own, and `universe.Flag` does not publish either.
            if ("METHOD".equals(name) || "CONSTRUCTOR".equals(name)) {
                continue;
            }
            // `universe.Flag` publishes the flags a macro may *build* with;
            // the ones only nsc's own namer sets (`ACCESSOR`) are read off
            // the internal table instead.
            if ("LOCAL".equals(name) || "ACCESSOR".equals(name)) {
                flags |= internalFlag(name);
            } else {
                flags |= flagValue(name);
            }
        }
        return flags;
    }

    static boolean hasFlagName(Sexp f, String want) {
        for (Sexp n : f.items.subList(1, f.items.size())) {
            if (want.equals(n.text())) {
                return true;
            }
        }
        return false;
    }

    static long flagValue(String name) throws Exception {
        Object flagValues = call(universe, "Flag", 0);
        for (Method m : flagValues.getClass().getMethods()) {
            if (m.getParameterCount() == 0 && m.getReturnType() == long.class
                    && m.getName().equals(name)) {
                m.setAccessible(true);
                return ((Number) m.invoke(flagValues)).longValue();
            }
        }
        throw new IllegalStateException("no reflect flag named " + name);
    }

    /** `Seq(xs)`: `newScopeWith` takes a varargs sequence, not a list. */
    static Object seq(List<Object> xs) throws Exception {
        return list(xs);
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
        Long orig = origTrees.get(t);
        if (orig != null) {
            // Its shape goes too: scala-rs uses it for a second mention.
            sb.append("(t \"Orig\" (s0) ").append(orig).append(' ');
            serTreeShape(t, sb);
            sb.append(')');
            return;
        }
        serTreeShape(t, sb);
    }

    static void serTreeShape(Object t, StringBuilder sb) throws Exception {
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
            Long sourceId = sourceSymbolIds.get(sym);
            if (sourceId != null) {
                sb.append("(sr ").append(sourceId).append(')');
                return;
            }
            if (sym == null || sym == call(universe, "NoSymbol", 0)) {
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

    /**
     * The type a `TypeTree` carries, as a class name applied to arguments.
     *
     * scala-rs rebuilds `(ty "a.b.C" <arg>...)` as the path `a.b.C` applied
     * to its arguments, so only a type that path really denotes may be
     * written that way: a class type, after aliases are expanded, whose class
     * is static. Anything else -- a singleton type, a refinement, an
     * existential, an abstract type or type parameter, a class nested in a
     * class -- is written as `(tyx "shown")`, which scala-rs refuses by name.
     * Its `typeSymbol` would otherwise name a *different* type: the
     * underlying class of `x.type`, the first parent of a refinement, the
     * bound of an abstract type.
     */
    static void serType(Object tpe, StringBuilder sb) throws Exception {
        if (tpe == null || tpe == call(universe, "NoType", 0)) {
            sb.append("(ty \"\")");
            return;
        }
        if (!isA(tpe, "scala.reflect.internal.Types$TypeRef")) {
            sb.append("(tyx ").append(Sexp.quote(String.valueOf(tpe))).append(')');
            return;
        }
        // Nothing here may ask a symbol for its info: a class the calling
        // run is compiling completes its info by asking scala-rs, which may
        // refuse ({@link #sourceSymbol}), and `isStatic`, `typeSymbol` and
        // `dealias` all read it. A class type is never an alias, so only a
        // non-class is dealiased, and staticness is read off the owner
        // chain's flags.
        Object d = tpe;
        Object sym = call(d, "typeSymbolDirect", 0);
        if (sourceSymbolIds.containsKey(sym) && Boolean.TRUE.equals(call(sym, "isClass", 0))) {
            // A class the calling run is compiling, which scala-rs sent as
            // its identity: it recognises it again by its full name, wherever
            // the class is nested (a table class inside a component trait has
            // no static path at all).
            sb.append("(ty ").append(Sexp.quote(String.valueOf(call(sym, "fullName", 0))));
            Object args = call(d, "typeArgs", 0);
            Object it = call(args, "iterator", 0);
            while ((Boolean) call(it, "hasNext", 0)) {
                sb.append(' ');
                serType(call(it, "next", 0), sb);
            }
            sb.append(')');
            return;
        }
        if (!Boolean.TRUE.equals(call(sym, "isClass", 0))) {
            d = call(tpe, "dealias", 0);
            if (!isA(d, "scala.reflect.internal.Types$TypeRef")) {
                sb.append("(tyx ").append(Sexp.quote(String.valueOf(tpe))).append(')');
                return;
            }
            sym = call(d, "typeSymbolDirect", 0);
        }
        if (!Boolean.TRUE.equals(call(sym, "isClass", 0))
                || Boolean.TRUE.equals(call(sym, "isModuleClass", 0))
                || Boolean.TRUE.equals(call(sym, "isRefinementClass", 0))
                || !staticByOwners(sym)) {
            // Named by the symbol's full name: printing the type could force
            // the info of a class whose description scala-rs refuses.
            sb.append("(tyx ").append(Sexp.quote(String.valueOf(call(sym, "fullName", 0))))
              .append(')');
            return;
        }
        String name = String.valueOf(call(sym, "fullName", 0));
        sb.append("(ty ").append(Sexp.quote(name));
        Object args = call(d, "typeArgs", 0);
        Object it = call(args, "iterator", 0);
        while ((Boolean) call(it, "hasNext", 0)) {
            sb.append(' ');
            serType(call(it, "next", 0), sb);
        }
        sb.append(')');
    }

    /** Every owner up to the root is a package or an object: `sym` has a path. */
    static boolean staticByOwners(Object sym) throws Exception {
        Object none = call(universe, "NoSymbol", 0);
        Object o = call(sym, "owner", 0);
        for (int i = 0; i < 64 && o != null && o != none; i++) {
            if (Boolean.TRUE.equals(call(o, "isRoot", 0))
                    || Boolean.TRUE.equals(call(o, "isEmptyPackageClass", 0))) {
                return true;
            }
            if (!Boolean.TRUE.equals(call(o, "isPackageClass", 0))
                    && !Boolean.TRUE.equals(call(o, "isModuleClass", 0))) {
                return false;
            }
            o = call(o, "owner", 0);
        }
        return false;
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

    /** Reset local bindings while preserving symbols defined outside this tree.
     * Mirrors nsc ResetAttrs: collect definitions first, then rebuild and clear
     * types. Rebuilding avoids mutating the caller's attributed tree.
     */
    static Object untypecheck(Object tree) throws Exception {
        java.util.Set<Object> locals = java.util.Collections.newSetFromMap(
            new java.util.IdentityHashMap<Object, Boolean>());
        collectLocalSymbols(tree, locals);
        return resetTree(tree, locals);
    }

    static void collectLocalSymbols(Object tree, java.util.Set<Object> locals) throws Exception {
        if (tree == call(universe, "EmptyTree", 0)) return;
        String kind = String.valueOf(call(tree, "productPrefix", 0));
        if (isA(tree, "scala.reflect.api.Trees$DefTreeApi")
                || "Function".equals(kind) || "Template".equals(kind)) {
            Object sym = call(tree, "symbol", 0);
            if (sym != null && sym != call(universe, "NoSymbol", 0)) {
                locals.add(sym);
            }
        }
        Object children = call(call(tree, "children", 0), "iterator", 0);
        while ((Boolean) call(children, "hasNext", 0)) collectLocalSymbols(call(children, "next", 0), locals);
    }

    static boolean mentionsLocal(Object tpe, java.util.Set<Object> locals) throws Exception {
        if (tpe == null || tpe == call(universe, "NoType", 0)) return false;
        if (locals.contains(call(tpe, "typeSymbol", 0))) return true;
        Object args = call(call(tpe, "typeArgs", 0), "iterator", 0);
        while ((Boolean) call(args, "hasNext", 0)) {
            if (mentionsLocal(call(args, "next", 0), locals)) return true;
        }
        return false;
    }

    static Object resetValue(Object value, java.util.Set<Object> locals) throws Exception {
        if (isA(value, "scala.reflect.api.Trees$TreeApi")) return resetTree(value, locals);
        if (isA(value, "scala.collection.immutable.List")) {
            List<Object> values = new ArrayList<>();
            Object it = call(value, "iterator", 0);
            while ((Boolean) call(it, "hasNext", 0)) values.add(resetValue(call(it, "next", 0), locals));
            return list(values);
        }
        return value;
    }

    static Object resetTree(Object tree, java.util.Set<Object> locals) throws Exception {
        if (tree == call(universe, "EmptyTree", 0)) return tree;
        String kind = String.valueOf(call(tree, "productPrefix", 0));
        if ("TypeTree".equals(kind)) {
            Object original = call(tree, "original", 0);
            if (original != null) return resetTree(original, locals);
            if (Boolean.TRUE.equals(call(tree, "wasEmpty", 0))
                    || mentionsLocal(call(tree, "tpe", 0), locals)) {
                return call(call(tree, "duplicate", 0), "clearType", 0);
            }
            return tree;
        }
        int arity = (Integer) call(tree, "productArity", 0);
        Object[] args = new Object[arity];
        for (int i = 0; i < arity; i++) args[i] = resetValue(call(tree, "productElement", 1, Integer.valueOf(i)), locals);
        if ("TypeApply".equals(kind)) {
            Object it = call(args[1], "iterator", 0);
            while ((Boolean) call(it, "hasNext", 0)) {
                if (Boolean.TRUE.equals(call(call(it, "next", 0), "isEmpty", 0))) return args[0];
            }
        }
        Object built = call(companion(kind), "apply", arity, args);
        if (Boolean.TRUE.equals(call(tree, "hasSymbolField", 0))) {
            Object sym = call(tree, "symbol", 0);
            if (sym != null && !locals.contains(sym)) call(built, "setSymbol", 1, sym);
        }
        call(built, "setAttachments", 1, call(tree, "attachments", 0));
        return built;
    }

    /** Mirror an actual source symbol. Its type is completed by reverse RPC,
     * not guessed while its enclosing definition is still being inferred.
     *
     * A package is the runtime mirror's own package of that name, so that a
     * class this run is compiling sits in a real scope beside its companion:
     * nsc finds a companion by looking its name up in the owner's
     * declarations (`Symbol.companionModule0`), and slick's `mapToImpl` asks
     * for exactly that. The runtime package's scope is looked up by name and
     * falls back to class loading only for a name nobody entered, so a class
     * entered here is found and a class file is never looked for. */
    static Object sourceSymbol(long id) throws Exception {
        Object known = sourceSymbols.get(id);
        if (known != null) return known;
        Sexp answer = query("(q symbol " + id + ")");
        String kind = answer.items.get(3).text();
        String name = answer.items.get(4).text();
        String full = answer.items.get(5).text();
        long parent = Long.parseLong(answer.items.get(6).text());
        if ("<root>".equals(name) || "<_root_>".equals(name) || "<empty>".equals(name)) {
            Object root = call(mirror, "EmptyPackageClass", 0);
            sourceSymbols.put(id, root);
            sourceSymbolIds.put(root, id);
            return root;
        }
        if ("Package".equals(kind)) {
            Object pkgClass = call(call(mirror, "staticPackage", 1, full), "moduleClass", 0);
            sourceSymbols.put(id, pkgClass);
            sourceSymbolIds.put(pkgClass, id);
            return pkgClass;
        }
        Object owner = parent == 0 ? call(mirror, "EmptyPackageClass", 0) : sourceSymbol(parent);
        known = sourceSymbols.get(id);
        if (known != null) return known;
        Object internal = call(universe, "internal", 0);
        Object pos = call(universe, "NoPosition", 0);
        long flags = flagsOf(answer.items.get(7));
        if ("ModuleClass".equals(kind) || "Module".equals(kind)) {
            return sourceModule(id, kind, name, full, owner, flags);
        }
        Object symbol;
        if ("Class".equals(kind)) {
            Object existing = synthetic.get(full);
            symbol = existing == null
                ? newSourceSymbol("ClassSymbol", owner, typeName(name), pos, flags)
                : call(existing, "typeSymbolDirect", 0);
        } else if ("Method".equals(kind)) {
            symbol = newSourceSymbol("MethodSymbol", owner, termName(name), pos, flags | internalFlag("METHOD"));
        } else if ("Term".equals(kind)) {
            symbol = newSourceSymbol("TermSymbol", owner, termName(name), pos, flags);
        } else {
            throw gap("macro mirror cannot describe source symbol kind " + kind);
        }
        sourceSymbols.put(id, symbol);
        sourceSymbolIds.put(symbol, id);
        if ("Class".equals(kind)) {
            synthetic.put(full, ownedTypeRef(owner, symbol));
        }
        Object lazy = lazyInfoConstructor().newInstance(universe, Long.valueOf(id));
        call(symbol, "setInfo", 1, lazy);
        if ("Class".equals(kind)) {
            enterInOwner(owner, symbol);
            long companion = Long.parseLong(query("(q companion " + id + ")").items.get(2).text());
            if (companion != 0) sourceSymbol(companion);
        }
        return symbol;
    }

    /** An object: its module symbol and its module class, made once, both
     * registered under their scala-rs identities. The module's info is the
     * module class's type; the module class's info is asked for lazily. */
    static Object sourceModule(long id, String kind, String name, String full, Object owner,
                               long flags) throws Exception {
        Sexp pair = query("(q modulePair " + id + ")");
        long moduleId = Long.parseLong(pair.items.get(2).text());
        long classId = Long.parseLong(pair.items.get(3).text());
        Object known = sourceSymbols.get(id);
        if (known != null) return known;
        Object internal = call(universe, "internal", 0);
        Object pos = call(universe, "NoPosition", 0);
        String simple = name.endsWith("$") ? name.substring(0, name.length() - 1) : name;
        flags |= internalFlag("MODULE");
        Object module = newSourceSymbol("ModuleSymbol", owner, termName(simple), pos, flags);
        Object moduleClass = newSourceSymbol("ModuleClassSymbol", owner, typeName(simple), pos,
            flags & internalFlag("ModuleToClassFlags"));
        call(universe, "connectModuleToClass", 2, module, moduleClass);
        sourceSymbols.put(moduleId, module);
        sourceSymbolIds.put(module, moduleId);
        sourceSymbols.put(classId, moduleClass);
        sourceSymbolIds.put(moduleClass, classId);
        Object classType = ownedTypeRef(owner, moduleClass);
        synthetic.put(full, classType);
        call(call(internal, "reificationSupport", 0), "setInfo", 2, module, classType);
        Object lazy = lazyInfoConstructor().newInstance(universe, Long.valueOf(classId));
        call(moduleClass, "setInfo", 1, lazy);
        enterInOwner(owner, module);
        long companion = Long.parseLong(query("(q companion " + moduleId + ")").items.get(2).text());
        if (companion != 0) sourceSymbol(companion);
        return "Module".equals(kind) ? module : moduleClass;
    }

    /** `owner.this.C`, or `C` with no prefix for a local class. */
    static Object ownedTypeRef(Object owner, Object symbol) throws Exception {
        Object internal = call(universe, "internal", 0);
        Object prefix = Boolean.TRUE.equals(call(owner, "isClass", 0))
            ? call(internal, "thisType", 1, owner) : call(universe, "NoPrefix", 0);
        return call(internal, "typeRef", 3, prefix, symbol, list(new ArrayList<>()));
    }

    /** Enter a class or an object into its package's scope, where nsc's
     * companion lookup and the mirror's own name lookup find it. Only a
     * package owner has a scope that outlives this exchange; a member of a
     * class is found through that class's info instead. */
    static void enterInOwner(Object owner, Object symbol) throws Exception {
        if (!Boolean.TRUE.equals(call(owner, "isPackageClass", 0))) return;
        Object decls = call(call(owner, "info", 0), "decls", 0);
        Object existing = call(decls, "lookup", 1, call(symbol, "name", 0));
        if (existing == symbol) return;
        call(decls, "enter", 1, symbol);
    }

    /** Compiler-owned symbols may change lexical owners. Runtime classpath
     * symbols must never be mutated. Scala's normal runtime owner setter refuses
     * all changes; these private symbol adapters implement that operation only for
     * the fresh source mirror symbols owned by this serial compiler exchange. */
    static Object newSourceSymbol(String kind, Object owner, Object name, Object pos, long flags) throws Exception {
        Class<?> adapter = sourceSymbolClasses.get(kind);
        if (adapter == null) {
            Object internal = call(universe, "internal", 0);
            Object prototype;
            if (kind.equals("ModuleSymbol") || kind.equals("ModuleClassSymbol")) {
                Object pair = call(internal, "newModuleAndClassSymbol", 4, owner, name, pos, Long.valueOf(flags));
                prototype = call(pair, kind.equals("ModuleSymbol") ? "_1" : "_2", 0);
            } else {
                String factory = kind.equals("MethodSymbol") ? "newMethodSymbol"
                    : kind.equals("TermSymbol") ? "newTermSymbol" : "newClassSymbol";
                prototype = call(internal, factory, 4, owner, name, pos, Long.valueOf(flags));
            }
            Class<?> template = prototype.getClass();
            if (!template.getName().startsWith("scala.reflect.runtime.SynchronizedSymbols$")) {
                throw gap("unsupported source-symbol runtime implementation " + template.getName());
            }
            byte[] data = synchronizedSymbolAdapter(template, "ScalaRsSource" + kind);
            class SourceLoader extends ClassLoader {
                SourceLoader() { super(macroCl); }
                Class<?> define(byte[] bytes) { return defineClass(null, bytes, 0, bytes.length); }
            }
            adapter = new SourceLoader().define(data);
            sourceSymbolClasses.put(kind, adapter);
        }
        Object symbol = adapter.getConstructors()[0].newInstance(owner, pos, name);
        mutableSourceSymbols.add(symbol);
        call(symbol, "setFlag", 1, Long.valueOf(flags));
        return symbol;
    }

    /** Preserve the runtime's actual synchronization mixins and constructor.
     * Its anonymous classes are final, so copy that classfile under a private
     * name and add exactly one owner setter. All other methods/fields remain
     * those of the loaded scala-reflect implementation. */
    static byte[] synchronizedSymbolAdapter(Class<?> template, String renamed) throws Exception {
        String original = template.getName().replace('.', '/');
        java.io.InputStream resource = template.getResourceAsStream("/" + original + ".class");
        if (resource == null) throw gap("missing source-symbol classfile " + original);
        try (java.io.DataInputStream in = new java.io.DataInputStream(resource)) {
            java.io.ByteArrayOutputStream buffer = new java.io.ByteArrayOutputStream();
            java.io.DataOutputStream d = new java.io.DataOutputStream(buffer);
            if (in.readInt() != 0xcafebabe) throw gap("invalid source-symbol classfile");
            d.writeInt(0xcafebabe); d.writeShort(in.readUnsignedShort()); d.writeShort(in.readUnsignedShort());
            int count = in.readUnsignedShort();
            if (count > 65526) throw gap("source-symbol constant pool is full");
            d.writeShort(count + 9);
            String[] strings = new String[count];
            for (int i = 1; i < count; i++) {
                int tag = in.readUnsignedByte(); d.writeByte(tag);
                switch (tag) {
                    case 1:
                        strings[i] = in.readUTF();
                        d.writeUTF(strings[i].replace(original, renamed));
                        break;
                    case 3: case 4: case 9: case 10: case 11: case 12: case 18: case 17:
                        d.writeInt(in.readInt()); break;
                    case 5: case 6:
                        d.writeLong(in.readLong()); i++; break;
                    case 7: case 8: case 16: case 19: case 20:
                        d.writeShort(in.readUnsignedShort()); break;
                    case 15:
                        d.writeByte(in.readUnsignedByte()); d.writeShort(in.readUnsignedShort()); break;
                    default: throw gap("unsupported source-symbol constant pool tag " + tag);
                }
            }
            utf(d, "owner_$eq"); utf(d, "(Lscala/reflect/internal/Symbols$Symbol;)V");
            utf(d, "ScalaRsMacroEngine"); pair(d, 7, count + 2, 0);
            utf(d, "changeSourceOwner"); utf(d, "(Ljava/lang/Object;Ljava/lang/Object;)V");
            pair(d, 12, count + 4, count + 5); pair(d, 10, count + 3, count + 6); utf(d, "Code");
            d.writeShort(in.readUnsignedShort()); d.writeShort(in.readUnsignedShort()); d.writeShort(in.readUnsignedShort());
            int interfaces = in.readUnsignedShort(); d.writeShort(interfaces);
            for (int i = 0; i < interfaces; i++) d.writeShort(in.readUnsignedShort());
            int fields = in.readUnsignedShort(); d.writeShort(fields);
            for (int i = 0; i < fields; i++) copyMember(in, d);
            int methods = in.readUnsignedShort();
            List<byte[]> kept = new ArrayList<>();
            for (int i = 0; i < methods; i++) {
                java.io.ByteArrayOutputStream method = new java.io.ByteArrayOutputStream();
                java.io.DataOutputStream m = new java.io.DataOutputStream(method);
                int access = in.readUnsignedShort(), name = in.readUnsignedShort(), desc = in.readUnsignedShort();
                m.writeShort(access); m.writeShort(name); m.writeShort(desc); copyAttributes(in, m);
                if (!("owner_$eq".equals(strings[name]) && "(Lscala/reflect/internal/Symbols$Symbol;)V".equals(strings[desc]))) kept.add(method.toByteArray());
            }
            d.writeShort(kept.size() + 1);
            for (byte[] method : kept) d.write(method);
            int target = count + 7;
            code(d, count, count + 1, count + 8, 2, 2,
                new byte[]{0x2a,0x2b,(byte)0xb8,(byte)(target >>> 8),(byte)target,(byte)0xb1});
            copyAttributes(in, d);
            d.flush(); return buffer.toByteArray();
        }
    }

    static void copyMember(java.io.DataInputStream in, java.io.DataOutputStream out) throws java.io.IOException {
        out.writeShort(in.readUnsignedShort()); out.writeShort(in.readUnsignedShort()); out.writeShort(in.readUnsignedShort());
        copyAttributes(in, out);
    }
    static void copyAttributes(java.io.DataInputStream in, java.io.DataOutputStream out) throws java.io.IOException {
        int count = in.readUnsignedShort(); out.writeShort(count);
        for (int i = 0; i < count; i++) {
            out.writeShort(in.readUnsignedShort()); int length = in.readInt();
            if (length < 0) throw new java.io.IOException("invalid classfile attribute length");
            byte[] data = new byte[length]; in.readFully(data); out.writeInt(length); out.write(data);
        }
    }

    public static void changeSourceOwner(Object symbol, Object next) throws Exception {
        synchronized (mutableSourceSymbols) {
            if (!mutableSourceSymbols.contains(symbol)) throw gap("cannot change a runtime classpath symbol's owner");
            // Match Symbol.owner_=, including originalOwner bookkeeping. The
            // actual reflection ChangeOwnerTraverser still performs the tree
            // traversal and also moves a module's class, as nsc does.
            call(universe, "saveOriginalOwner", 1, symbol);
            java.lang.reflect.Field owner = Class.forName("scala.reflect.internal.Symbols$Symbol", true, macroCl)
                .getDeclaredField("_rawowner");
            owner.setAccessible(true);
            owner.set(symbol, next);
        }
    }

    static long internalFlag(String name) throws Exception {
        Object flags = Class.forName("scala.reflect.internal.Flags$", true, macroCl).getField("MODULE$").get(null);
        return ((Number) call(flags, name, 0)).longValue();
    }

    /**
     * The views of a scala-rs `val` a class declaration was described with
     * (getter, setter, field), by the negative code their lazy info carries.
     * nsc makes two or three symbols of one `val`; each one's info is asked
     * for as that view of the one scala-rs symbol.
     */
    static final java.util.Map<Long, Object[]> viewCodes = new java.util.HashMap<>();
    static long nextViewCode = -1;

    /** Called by the generated LazyType subclass when reflection forces info. */
    public static void completeMirrorSymbol(long id, Object symbol) throws Exception {
        Sexp answer;
        if (id < 0) {
            Object[] view = viewCodes.get(id);
            answer = query("(q viewInfo " + view[0] + " " + view[1] + ")");
        } else {
            answer = query("(q symbolInfo " + id + ")");
        }
        Object info = sourceInfo(symbol, answer.items.get(2));
        call(symbol, "setInfo", 1, info);
    }

    /** A declaration of a class described lazily (`crate::expand_mirror`). */
    static Object lazyDecl(Object owner, Sexp d) throws Exception {
        String form = d.items.get(0).atom;
        Object pos = call(universe, "NoPosition", 0);
        if ("dm".equals(form)) {
            long id = Long.parseLong(d.items.get(1).text());
            String name = d.items.get(2).text();
            long flags = flagsOf(d.items.get(3));
            Object known = sourceSymbols.get(id);
            if (known != null && call(known, "owner", 0) == owner) {
                call(known, "setFlag", 1, Long.valueOf(flags));
                return known;
            }
            Object sym = newSourceSymbol("MethodSymbol", owner, termName(name), pos,
                flags | internalFlag("METHOD"));
            if (known == null) {
                sourceSymbols.put(id, sym);
                sourceSymbolIds.put(sym, id);
            }
            call(sym, "setInfo", 1, lazyInfoConstructor().newInstance(universe, Long.valueOf(id)));
            return sym;
        }
        if ("dv".equals(form)) {
            long id = Long.parseLong(d.items.get(1).text());
            String name = d.items.get(2).text();
            String view = d.items.get(3).atom;
            long flags = flagsOf(d.items.get(4));
            Object sym = "field".equals(view)
                ? newSourceSymbol("TermSymbol", owner, termName(name), pos, flags)
                : newSourceSymbol("MethodSymbol", owner, termName(name), pos,
                    flags | internalFlag("METHOD"));
            if (!sourceSymbols.containsKey(id)) {
                sourceSymbols.put(id, sym);
                sourceSymbolIds.put(sym, id);
            }
            long code = nextViewCode--;
            viewCodes.put(code, new Object[]{view, Long.valueOf(id)});
            call(sym, "setInfo", 1, lazyInfoConstructor().newInstance(universe, Long.valueOf(code)));
            return sym;
        }
        if ("de".equals(form)) {
            String name = d.items.get(1).text();
            long flags = flagsOf(d.items.get(2));
            Object sym = newSourceSymbol("MethodSymbol", owner, termName(name), pos,
                flags | internalFlag("METHOD"));
            call(sym, "setInfo", 1, sourceInfo(sym, d.items.get(3)));
            return sym;
        }
        throw gap("the macro mirror cannot read the declaration " + d);
    }

    static Object sourceInfo(Object owner, Sexp s) throws Exception {
        String kind = s.items.get(0).text();
        Object internal = call(universe, "internal", 0);
        if ("notype".equals(kind)) return call(universe, "NoType", 0);
        if ("nullary".equals(kind)) return call(internal, "nullaryMethodType", 1, sourceInfo(owner, s.items.get(1)));
        if ("method".equals(kind)) {
            List<Object> params = new ArrayList<>();
            for (Sexp arg : s.items.get(1).items.subList(1, s.items.get(1).items.size())) {
                if ("argn".equals(arg.items.get(0).atom)) {
                    // A parameter with no scala-rs symbol of its own: nsc's
                    // `x$1`, or a case constructor's parameter, whose symbol
                    // in scala-rs is the field it initialises.
                    Object param = newSourceSymbol("TermSymbol", owner,
                        termName(arg.items.get(1).text()), call(universe, "NoPosition", 0),
                        flagsOf(arg.items.get(2)) | flagValue("PARAM"));
                    call(param, "setInfo", 1, typeFor(arg.items.get(3)));
                    params.add(param);
                    continue;
                }
                Object param = sourceSymbol(Long.parseLong(arg.items.get(1).text()));
                call(param, "setInfo", 1, typeFor(arg.items.get(2)));
                params.add(param);
            }
            return call(internal, "methodType", 2, list(params), sourceInfo(owner, s.items.get(2)));
        }
        if ("selftype".equals(kind)) {
            // An object's constructor returns the object's own type.
            Object cls = call(owner, "owner", 0);
            return ownedTypeRef(call(cls, "owner", 0), cls);
        }
        if ("classinfo".equals(kind)) {
            List<Object> parents = new ArrayList<>();
            for (Sexp ty : s.items.get(1).items.subList(1, s.items.get(1).items.size())) parents.add(typeFor(ty));
            List<Object> decls = new ArrayList<>();
            for (Sexp decl : s.items.get(2).items.subList(1, s.items.get(2).items.size())) {
                decls.add(lazyDecl(owner, decl));
            }
            return call(internal, "classInfoType", 3, list(parents), call(internal, "newScopeWith", 1, seq(decls)), owner);
        }
        return typeFor(s);
    }

    /** A small Java-5 classfile lets the reflection-only bridge subclass
     * Scala's LazyType without depending on Scala jars at javac time. It only
     * stores a source ID and forwards completion; no type is fabricated. */
    static Constructor<?> lazyInfoConstructor() throws Exception {
        if (lazyInfoClass == null) {
            java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
            java.io.DataOutputStream d = new java.io.DataOutputStream(bytes);
            d.writeInt(0xcafebabe); d.writeShort(0); d.writeShort(49); d.writeShort(25);
            utf(d, "ScalaRsMacroLazyInfo"); pair(d, 7, 1, 0);
            utf(d, "scala/reflect/internal/Types$LazyType"); pair(d, 7, 3, 0);
            utf(d, "scala/reflect/internal/Types$FlagAgnosticCompleter"); pair(d, 7, 5, 0);
            utf(d, "id"); utf(d, "J"); utf(d, "<init>");
            utf(d, "(Lscala/reflect/internal/SymbolTable;J)V"); utf(d, "Code");
            utf(d, "(Lscala/reflect/internal/SymbolTable;)V"); pair(d, 12, 9, 12); pair(d, 10, 4, 13);
            pair(d, 12, 7, 8); pair(d, 9, 2, 15);
            utf(d, "complete"); utf(d, "(Lscala/reflect/internal/Symbols$Symbol;)V");
            utf(d, "ScalaRsMacroEngine"); pair(d, 7, 19, 0);
            utf(d, "completeMirrorSymbol"); utf(d, "(JLjava/lang/Object;)V"); pair(d, 12, 21, 22); pair(d, 10, 20, 23);
            d.writeShort(0x31); d.writeShort(2); d.writeShort(4);
            d.writeShort(1); d.writeShort(6);
            d.writeShort(1); d.writeShort(0x12); d.writeShort(7); d.writeShort(8); d.writeShort(0);
            d.writeShort(2);
            code(d, 9, 10, 3, 4, new byte[]{0x2a,0x2b,(byte)0xb7,0,14,0x2a,0x20,(byte)0xb5,0,16,(byte)0xb1});
            code(d, 17, 18, 3, 2, new byte[]{0x2a,(byte)0xb4,0,16,0x2b,(byte)0xb8,0,24,(byte)0xb1});
            d.writeShort(0); d.flush();
            class AdapterLoader extends ClassLoader {
                AdapterLoader() { super(macroCl); }
                Class<?> define(byte[] data) { return defineClass(null, data, 0, data.length); }
            }
            lazyInfoClass = new AdapterLoader().define(bytes.toByteArray());
        }
        return lazyInfoClass.getConstructors()[0];
    }

    static void utf(java.io.DataOutputStream d, String text) throws java.io.IOException { d.writeByte(1); d.writeUTF(text); }
    static void pair(java.io.DataOutputStream d, int tag, int a, int b) throws java.io.IOException {
        d.writeByte(tag); d.writeShort(a); if (tag != 7) d.writeShort(b);
    }
    static void code(java.io.DataOutputStream d, int name, int desc, int stack, int locals, byte[] bytes) throws java.io.IOException {
        code(d, name, desc, 11, stack, locals, bytes);
    }
    static void code(java.io.DataOutputStream d, int name, int desc, int codeName, int stack, int locals, byte[] bytes) throws java.io.IOException {
        d.writeShort(1); d.writeShort(name); d.writeShort(desc); d.writeShort(1);
        d.writeShort(codeName); d.writeInt(12 + bytes.length); d.writeShort(stack); d.writeShort(locals);
        d.writeInt(bytes.length); d.write(bytes); d.writeShort(0); d.writeShort(0);
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
        Object internalProxy;

        public Object invoke(Object proxy, Method m, Object[] a) throws Throwable {
            String n = m.getName();
            int arity = m.getParameterCount();
            if (n.equals("internal") && arity == 0) {
                if (internalProxy == null) {
                    Class<?> api = Class.forName("scala.reflect.macros.Internals$ContextInternalApi", true, macroCl);
                    internalProxy = Proxy.newProxyInstance(macroCl, new Class<?>[]{api}, (p, method, args) -> {
                        if (method.getName().equals("enclosingOwner") && method.getParameterCount() == 0) {
                            Sexp answer = query("(q enclosingOwner)");
                            return sourceSymbol(Long.parseLong(answer.items.get(2).text()));
                        }
                        if (method.getName().equals("scala$reflect$macros$Internals$ContextInternalApi$$$outer")) return proxy;
                        Object implementation = call(universe, "internal", 0);
                        try {
                            return call(implementation, method.getName(), method.getParameterCount(),
                                args == null ? new Object[0] : args);
                        } catch (InvocationTargetException wrapped) {
                            throw wrapped.getCause();
                        } catch (NoSuchMethodException missing) {
                            throw gap("Context.internal." + method.getName() + " is not implemented");
                        }
                    });
                }
                return internalProxy;
            }
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
            if ((n.equals("untypecheck") || n.equals("resetLocalAttrs")) && arity == 1) {
                return untypecheck(a[0]);
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
            // `c.typecheck` and the three modes it takes.
            //
            // nsc's modes are `analyzer.Mode` values, an internal bitset this
            // engine has no access to and no use for: the only thing it does
            // with a mode is send its name to scala-rs. So the marker *is* the
            // name, and a mode that did not come from one of these three is
            // refused by name rather than read as TERMmode
            // ({@link #typecheck}).
            if (arity == 0 && (n.equals("TERMmode") || n.equals("TYPEmode")
                    || n.equals("PATTERNmode"))) {
                return n.substring(0, n.length() - "mode".length());
            }
            if (n.equals("typecheck") && arity == 6) {
                return typecheck(a[0], a[1], a[2], (Boolean) a[3], (Boolean) a[4],
                    (Boolean) a[5]);
            }
            if (n.equals("TypecheckException") && arity == 0) {
                return Class.forName("scala.reflect.macros.TypecheckException$", true, macroCl)
                    .getField("MODULE$").get(null);
            }
            // Diagnostics and source inspection use the same call-site point.
            if (n.equals("enclosingPosition") && arity == 0) {
                return appTree == null ? call(universe, "NoPosition", 0) : call(appTree, "pos", 0);
            }
            if (m.isDefault()) {
                return invokeDefault(proxy, m, a);
            }
            throw new UnsupportedOperationException(
                "scala-rs macro engine: Context." + n + " is not implemented");
        }
    }

    /**
     * Invoke a default interface method without requiring a post-Java-8 API.
     *
     * InvocationHandler.invokeDefault was added in Java 16.  The macro
     * engine is intentionally compiled for Java 8 so a class generated by a
     * newer JDK can run under the JDK selected by the compiler process.  Use
     * the public API when it exists, privateLookupIn on Java 9--15, and the
     * Java 8 Lookup constructor on the oldest runtime.
     */
    static Object invokeDefault(Object proxy, Method method, Object[] args) throws Throwable {
        try {
            Method modern = InvocationHandler.class.getMethod(
                "invokeDefault", Object.class, Method.class, Object[].class);
            return modern.invoke(null, proxy, method, args);
        } catch (NoSuchMethodException absentOnJava8) {
            try {
                // privateLookupIn was added in Java 9.  Resolve it by
                // reflection so javac --release 8 can still compile this
                // source, then use the regular Lookup API on the result.
                Method privateLookupIn = MethodHandles.class.getMethod(
                    "privateLookupIn", Class.class, MethodHandles.Lookup.class);
                MethodHandles.Lookup lookup = (MethodHandles.Lookup) privateLookupIn.invoke(
                    null, method.getDeclaringClass(), MethodHandles.lookup());
                return lookup.unreflectSpecial(method, method.getDeclaringClass())
                    .bindTo(proxy)
                    .invokeWithArguments(args == null ? new Object[0] : args);
            } catch (NoSuchMethodException absentOnJava8Too) {
                // Java 8 has no privateLookupIn.  Its Lookup constructor is
                // the only available way to obtain interface private access.
                Constructor<MethodHandles.Lookup> constructor =
                    MethodHandles.Lookup.class.getDeclaredConstructor(Class.class, int.class);
                constructor.setAccessible(true);
                int allModes = MethodHandles.Lookup.PUBLIC | MethodHandles.Lookup.PRIVATE
                    | MethodHandles.Lookup.PROTECTED | MethodHandles.Lookup.PACKAGE;
                MethodHandles.Lookup lookup = constructor.newInstance(
                    method.getDeclaringClass(), allModes);
                return lookup.unreflectSpecial(method, method.getDeclaringClass())
                    .bindTo(proxy)
                    .invokeWithArguments(args == null ? new Object[0] : args);
            }
        } catch (InvocationTargetException wrapped) {
            throw wrapped.getCause();
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
        while ((t instanceof InvocationTargetException || t instanceof java.lang.reflect.UndeclaredThrowableException)
                && t.getCause() != null) {
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
