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
     * A type descriptor as a `universe.WeakTypeTag`.
     *
     * `(ty "java.lang.String")` is a class the mirror can find on the macro
     * classpath. `(syn "Pkg.Outer.Local")` is one this compilation run is
     * itself defining, for which there is no class file yet -- see
     * {@link #synthType}.
     */
    static Object buildTag(Sexp s) throws Exception {
        return tagOf(typeFor(s));
    }

    /**
     * The `universe.Type` a type descriptor names.
     *
     * `(ty "a.b.C" <arg>…)` is a class the mirror finds on the macro
     * classpath, applied to its type arguments; `(syn "a.b.C")` is one this
     * run is compiling ({@link #synthType}); `(cst (c "Int" "1"))` is the
     * constant type nsc gives a literal, which `c.typecheck(q"1").tpe` has to
     * be if it is to be the type nsc reports.
     */
    static Object typeFor(Sexp s) throws Exception {
        String head = s.items.get(0).atom;
        if ("cst".equals(head)) {
            return call(call(universe, "internal", 0), "constantType", 1,
                constant(s.items.get(1)));
        }
        String name = s.items.get(1).text();
        if ("syn".equals(head)) {
            return synthType(name);
        }
        if ("run".equals(head)) {
            return runClassType(s);
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
     * keyed by the full name scala-rs sent. One per name per engine process,
     * so the same class is always the same symbol and an expansion that
     * mentions it twice mentions one type.
     */
    static final java.util.HashMap<String, Object> synthetic = new java.util.HashMap<>();

    /**
     * A placeholder symbol for a class this run is compiling.
     *
     * The mirror resolves a class *by name against the macro classpath*, so a
     * class whose class file does not exist yet -- gitbucket's
     * `TableQuery[Issues]`, where `Issues` is declared a few lines away -- can
     * never be reached that way. nsc has no such problem: it expands in its
     * own universe, where the symbol is the very one the typer is building.
     *
     * What is built here carries the class's **identity and nothing else**:
     * its name, and no info at all. That is deliberate, and it is why the
     * symbol is safe to hand over. scala-rs cannot describe the class truly at
     * this point in its own run -- while `lazy val Issues = TableQuery[Issues]`
     * is being typed, the members of `class Issues` are still un-inferred --
     * so an info here would be a guess. Leaving it unset means an
     * implementation that asks for one gets an exception, which scala-rs turns
     * into a diagnostic, rather than a quiet wrong answer.
     *
     * The name is the class's real full name, which is how scala-rs recognises
     * the type again in the tree that comes back.
     */
    static Object synthType(String fullName) throws Exception {
        Object known = synthetic.get(fullName);
        if (known != null) {
            return known;
        }
        Object internal = call(universe, "internal", 0);
        Object owner = call(mirror, "EmptyPackageClass", 0);
        Object sym = call(internal, "newClassSymbol", 4,
            owner, typeName(fullName), call(universe, "NoPosition", 0), Long.valueOf(0L));
        // `sym.toType` would ask for the type parameters, and that completes
        // the symbol; the type reference is built directly so that the info
        // stays unset and any real question about the class still throws.
        Object tpe = call(internal, "typeRef", 3,
            call(internal, "thisType", 1, owner), sym, list(new ArrayList<>()));
        synthetic.put(fullName, tpe);
        return tpe;
    }

    /**
     * A class the calling run is compiling, **described**.
     *
     * {@link #synthType} builds the same symbol with no info at all, which is
     * all scala-rs could offer before the bridge could ask questions
     * backwards (`docs/macros.md` §5.1): identity and nothing else, so that an
     * implementation asking a real question got an exception instead of a
     * quiet wrong answer. It got one for `tpe.toString` too, because the
     * reflect internals need an info to print a type at all.
     *
     * scala-rs now sends the description with the name, and it does so only
     * when it can describe the class *completely* -- every parent, every
     * declared member, every one of their types. A `(syn ...)` still arrives
     * whenever it cannot, and that stays the empty placeholder. So there is no
     * middle state here: the symbol is either fully described or not described
     * at all, and this method is only reached in the first case.
     *
     * The symbol is cached under the same key as {@link #synthType}'s, so a
     * class named twice in one expansion is one symbol. A name that arrived
     * first as an empty placeholder is completed in place rather than
     * duplicated -- completing a symbol that had no info is monotone, and two
     * symbols for one class would break every identity comparison an
     * implementation makes.
     */
    static Object runClassType(Sexp s) throws Exception {
        String fullName = s.items.get(1).text();
        Object known = synthetic.get(fullName);
        Object internal = call(universe, "internal", 0);
        Object sym;
        Object tpe;
        if (known != null) {
            tpe = known;
            sym = call(tpe, "typeSymbol", 0);
            if (Boolean.TRUE.equals(call(sym, "isInitialized", 0))) {
                return tpe;
            }
        } else {
            Object owner = call(mirror, "EmptyPackageClass", 0);
            sym = call(internal, "newClassSymbol", 4, owner, typeName(fullName),
                call(universe, "NoPosition", 0), flagsOf(s.items.get(2)));
            tpe = call(internal, "typeRef", 3, call(internal, "thisType", 1, owner), sym,
                list(new ArrayList<>()));
            synthetic.put(fullName, tpe);
        }
        Object support = call(internal, "reificationSupport", 0);
        List<Object> parents = new ArrayList<>();
        Sexp ps = s.field("parents");
        for (Sexp x : ps.items.subList(1, ps.items.size())) {
            parents.add(typeFor(x));
        }
        List<Object> decls = new ArrayList<>();
        Sexp ds = s.field("decls");
        for (Sexp d : ds.items.subList(1, ds.items.size())) {
            decls.add(declSymbol(sym, tpe, d));
        }
        Object scope = call(internal, "newScopeWith", 1, seq(decls));
        Object info = call(internal, "classInfoType", 3, list(parents), scope, sym);
        call(support, "setInfo", 2, sym, info);
        return tpe;
    }

    /**
     * One declared member of such a class.
     *
     * The constructor is spelled out rather than described: scala-rs models it
     * as returning `Unit` with no parameter clause and nsc as returning the
     * class with one, so the wire carries the marker and the *class's own
     * type* is filled in here, where it is to hand.
     */
    static Object declSymbol(Object owner, Object ownerType, Sexp d) throws Exception {
        String name = d.items.get(1).text();
        Sexp flagNames = d.items.get(2);
        long flags = flagsOf(flagNames);
        boolean isMethod = hasFlagName(flagNames, "METHOD");
        boolean isCtor = hasFlagName(flagNames, "CONSTRUCTOR");
        Object internal = call(universe, "internal", 0);
        Object support = call(internal, "reificationSupport", 0);
        Sexp shape = d.items.get(3);
        boolean nullary = "nullary".equals(shape.items.get(0).atom);
        Object result = isCtor ? ownerType : typeFor(d.items.get(4));
        Object sym = isMethod
            ? call(internal, "newMethodSymbol", 4, owner, termName(name),
                call(universe, "NoPosition", 0), Long.valueOf(flags))
            : call(internal, "newTermSymbol", 4, owner, termName(name),
                call(universe, "NoPosition", 0), Long.valueOf(flags));
        Object info;
        if (!isMethod) {
            info = result;
        } else if (nullary) {
            info = call(internal, "nullaryMethodType", 1, result);
        } else {
            List<Object> params = new ArrayList<>();
            int i = 0;
            for (Sexp pt : shape.items.subList(1, shape.items.size())) {
                Object p = call(internal, "newTermSymbol", 4, sym, termName("x$" + (++i)),
                    call(universe, "NoPosition", 0), Long.valueOf(flagValue("PARAM")));
                call(support, "setInfo", 2, p, typeFor(pt));
                params.add(p);
            }
            info = call(internal, "methodType", 2, list(params), result);
        }
        return call(support, "setInfo", 2, sym, info);
    }

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
            flags |= flagValue(name);
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
        if (tpe == null || tpe == call(universe, "NoType", 0)) {
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
