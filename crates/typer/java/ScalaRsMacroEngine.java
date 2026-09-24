import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.lang.invoke.MethodHandles;
import java.lang.reflect.Constructor;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
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
    static final java.util.IdentityHashMap<Object, String> binaryClassNames = new java.util.IdentityHashMap<>();
    static final java.util.Set<Object> binarySymbols = java.util.Collections.newSetFromMap(new java.util.IdentityHashMap<>());
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
    /** Proxies retained only while their expansion is active in the typer. */
    static final java.util.Map<Long, Object> macroContexts = new java.util.HashMap<>();
    /** The run's source files by file index, sent once each: `(position src
     *  idx text path point)` defines one, `(position src idx path point)`
     *  reuses it. */
    static final java.util.Map<Integer, Object> sourceFiles = new java.util.HashMap<>();
    /** `c.openImplicits` entries by the handle scala-rs gave them. */
    static final java.util.Map<Long, Sexp> openImplicitEntries = new java.util.HashMap<>();
    static List<Object> openMacroContexts = new ArrayList<>();
    /**
     * The pipe, as fields, because expansion is a *conversation* rather than
     * one line in and one line out: an implementation may stop mid-flight and
     * ask scala-rs a question ({@link #query}), which is written and read on
     * the same two streams the request came in on.
     */
    static WireReader in;
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
    /**
     * Nanoseconds spent inside `Method.invoke` -- the macro implementation's own
     * run -- and, of that, the part spent blocked on an answer from scala-rs.
     * Read and reset by the `(timing)` request, which scala-rs sends after each
     * expansion when `SCALA_RS_MACRO_TIMING` is set. The difference is what the
     * JVM actually computed, which is the only number scala-rs cannot measure
     * from its own side of the pipe.
     */
    static long invokeNanos = 0;
    static long waitNanos = 0;
    /** Nanoseconds inside {@link #handle}: the whole exchange, of which
     * `invokeNanos` is the implementation's own run. */
    static long handleNanos = 0;
    static final int MAX_WIRE_CHARS = 16 * 1024 * 1024;
    static final int MAX_WIRE_DEPTH = 512;

    public static void main(String[] args) throws Exception {
        if (args.length == 1 && "--protocol-self-test".equals(args[0])) {
            protocolSelfTest();
            return;
        }
        if (args.length != 0) {
            throw new IllegalArgumentException("unexpected macro engine argument");
        }
        out = new PrintStream(System.out, true, "UTF-8");
        in = new WireReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        // Keep the original stream for protocol packets; macro println/Console
        // output is payload, never another expansion reply.
        System.setOut(new PrintStream(new MacroOutput("stdout"), true, "UTF-8"));
        System.setErr(new PrintStream(new MacroOutput("stderr"), true, "UTF-8"));
        ClassLoader baseCl = ScalaRsMacroEngine.class.getClassLoader();
        macroCl = baseCl;
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
        // Keep the runtime universe on the application loader. Reification
        // records its mirror and refuses to migrate trees between mirrors;
        // only loading the ScalaTest implementation needs the child-first
        // compatibility loader.
        macroCl = macroClassLoader(baseCl);
        out.println("(ready)");
        String line;
        while ((line = readWireLine(in)) != null) {
            if (line.isEmpty()) {
                continue;
            }
            String reply;
            long handleStarted = System.nanoTime();
            try {
                reply = handle(line);
            } catch (Throwable t) {
                reply = err(describe(t));
            }
            handleNanos += System.nanoTime() - handleStarted;
            out.println(reply);
        }
    }

    /**
     * ScalaTest's assertion macros finish by calling scalactic's
     * `MacroOwnerRepair`.  That helper casts the public macro `Context` to
     * nsc's private concrete `scala.reflect.macros.contexts.Context` solely to
     * obtain the call-site owner.  A scala-rs context deliberately implements
     * the public interface through a proxy, so the cast cannot ever succeed.
     *
     * The expansion crosses back into scala-rs and is typechecked at the real
     * call site before it is accepted.  Consequently nsc's in-JVM owner repair
     * is both unavailable and redundant here.  Load only the ScalaTest classes
     * that reach the helper in a child loader and replace that helper with the
     * ABI-equivalent identity operation. Shapeless's lazy derivation helper
     * similarly redirects its private nsc implicit-cache reset to the native
     * typer. All other macro classes and Scala reflection stay parent-loaded.
     */
    static ClassLoader macroClassLoader(ClassLoader parent) throws Exception {
        String[] entries = System.getProperty("java.class.path", "")
            .split(java.util.regex.Pattern.quote(java.io.File.pathSeparator));
        List<java.net.URL> urls = new ArrayList<>();
        for (String entry : entries) {
            if (!entry.isEmpty()) urls.add(new java.io.File(entry).toURI().toURL());
        }
        return new ScalaTestCompatLoader(urls.toArray(new java.net.URL[urls.size()]), parent);
    }

    static final class ScalaTestCompatLoader extends java.net.URLClassLoader {
        ScalaTestCompatLoader(java.net.URL[] urls, ClassLoader parent) {
            super(urls, parent);
        }

        static boolean childFirst(String name) {
            return name.startsWith("org.scalatest.AssertionsMacro")
                || name.startsWith("org.scalactic.BooleanMacro")
                || name.startsWith("org.scalactic.MacroOwnerRepair")
                || name.equals("shapeless.LazyMacros") || name.startsWith("shapeless.LazyMacros$");
        }

        @Override
        protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
            if (!childFirst(name)) return super.loadClass(name, resolve);
            synchronized (getClassLoadingLock(name)) {
                Class<?> cls = findLoadedClass(name);
                if (cls == null) {
                    if (name.equals("org.scalactic.MacroOwnerRepair")) {
                        byte[] bytes;
                        try {
                            bytes = ownerRepairBytes();
                        } catch (java.io.IOException e) {
                            throw new ClassNotFoundException(name, e);
                        }
                        cls = defineClass(name, bytes, 0, bytes.length);
                    } else if (name.equals("shapeless.LazyMacros$")
                            || name.equals("shapeless.LazyMacros$DerivationContext$State$")
                            || name.equals("shapeless.LazyMacros$SubstMessage$1")
                            || name.equals("shapeless.LazyMacros$DerivationContext$StripUnApplyNodes")) {
                        try (java.io.InputStream stream = getParent().getResourceAsStream(name.replace('.', '/') + ".class")) {
                            if (stream == null) throw new ClassNotFoundException(name);
                            java.io.ByteArrayOutputStream raw = new java.io.ByteArrayOutputStream();
                            byte[] buffer = new byte[8192]; int read;
                            while ((read = stream.read(buffer)) != -1) raw.write(buffer, 0, read);
                            byte[] bytes = name.equals("shapeless.LazyMacros$")
                                ? compilerApiBytes(raw.toByteArray(), false)
                                : name.equals("shapeless.LazyMacros$DerivationContext$State$")
                                    ? compilerApiBytes(raw.toByteArray(), true)
                                    : reflectionUniverseBytes(raw.toByteArray());
                            cls = defineClass(name, bytes, 0, bytes.length);
                        } catch (Exception failure) {
                            throw new ClassNotFoundException(name, failure);
                        }
                    } else {
                        try {
                            cls = findClass(name);
                        } catch (ClassNotFoundException missing) {
                            cls = super.loadClass(name, false);
                        }
                    }
                }
                if (resolve) resolveClass(cls);
                return cls;
            }
        }
    }

    /** These two tree transformers only use the common SymbolTable reflection
     * API (one stores the universe; the other reads standard term names).
     * Keep their original transformation logic and use that actual superclass
     * instead of requiring the unrelated nsc Global subclass. */
    static byte[] reflectionUniverseBytes(byte[] original) throws Exception {
        java.io.DataInputStream in = new java.io.DataInputStream(new java.io.ByteArrayInputStream(original));
        java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
        java.io.DataOutputStream out = new java.io.DataOutputStream(bytes);
        out.writeInt(in.readInt()); out.writeShort(in.readUnsignedShort()); out.writeShort(in.readUnsignedShort());
        int count = in.readUnsignedShort(); out.writeShort(count);
        for (int i = 1; i < count; i++) {
            int tag = in.readUnsignedByte(); out.writeByte(tag);
            switch (tag) {
                case 1:
                    out.writeUTF(in.readUTF().replace("scala/tools/nsc/Global", "scala/reflect/internal/SymbolTable"));
                    break;
                case 7: case 8: case 16: case 19: case 20: out.writeShort(in.readUnsignedShort()); break;
                case 9: case 10: case 11: case 12: case 18: case 17:
                    out.writeShort(in.readUnsignedShort()); out.writeShort(in.readUnsignedShort()); break;
                case 3: case 4: out.writeInt(in.readInt()); break;
                case 5: case 6: out.writeLong(in.readLong()); i++; break;
                case 15: out.writeByte(in.readUnsignedByte()); out.writeShort(in.readUnsignedShort()); break;
                default: throw gap("unsupported transformer constant " + tag);
            }
        }
        byte[] rest = new byte[in.available()]; in.readFully(rest); out.write(rest); out.flush();
        return bytes.toByteArray();
    }

    /** Move nsc's implicit-cache reset to the typer that owns implicit search. */
    public static void resetImplicitCaches(Object context) throws Exception {
        if (!Proxy.isProxyClass(context.getClass()) || !(Proxy.getInvocationHandler(context) instanceof Ctx))
            throw gap("implicit reset requires a scala-rs macro context");
        Sexp answer = query("(q resetImplicits)");
        if (!"reset".equals(answer.items.get(1).text())) throw gap("malformed implicit reset answer");
    }

    /** Read the annotation through the public reflection API. The package
     * helper uses nsc's analyzer even though only a formatted string is needed. */
    public static String implicitNotFoundMessage(Object receiver, Object context, Object type) throws Exception {
        Object symbol = call(type, "typeSymbol", 0);
        Object annotations = call(call(symbol, "annotations", 0), "iterator", 0);
        while ((Boolean) call(annotations, "hasNext", 0)) {
            Object annotation = call(annotations, "next", 0);
            Object tree = call(annotation, "tree", 0);
            Object annotationSymbol = call(call(tree, "tpe", 0), "typeSymbol", 0);
            if (!"scala.annotation.implicitNotFound".equals(call(annotationSymbol, "fullName", 0))) continue;
            Object arguments = call(tree, "args", 0);
            if ((Boolean) call(arguments, "isEmpty", 0)) continue;
            Object argument = call(arguments, "head", 0);
            if (!isA(argument, "scala.reflect.api.Trees$LiteralApi")) continue;
            Object value = call(call(argument, "value", 0), "value", 0);
            if (!(value instanceof String)) continue;
            String message = (String) value;
            Object parameters = call(call(symbol, "typeParams", 0), "iterator", 0);
            Object typeArguments = call(call(type, "typeArgs", 0), "iterator", 0);
            while ((Boolean) call(parameters, "hasNext", 0)) {
                String name = call(call(parameters, "next", 0), "name", 0).toString();
                String shown = (Boolean) call(typeArguments, "hasNext", 0)
                    ? call(typeArguments, "next", 0).toString() : name;
                message = message.replace("${" + name + "}", shown);
            }
            return message;
        }
        return "Implicit value of type " + type + " not found";
    }

    /** Preserve macro code except for a supported private-compiler call. The
     * diagnostic bridge consumes the original package receiver as well, so the
     * invocation keeps the same stack shape and bytecode length. */
    static byte[] compilerApiBytes(byte[] original, boolean diagnostic) throws Exception {
        java.io.DataInputStream input = new java.io.DataInputStream(new java.io.ByteArrayInputStream(original));
        input.readInt(); input.readUnsignedShort(); input.readUnsignedShort();
        int count = input.readUnsignedShort();
        String[] text = new String[count];
        int[] left = new int[count], right = new int[count];
        for (int i = 1; i < count; i++) {
            int tag = input.readUnsignedByte();
            if (tag == 1) text[i] = input.readUTF();
            else if (tag == 7 || tag == 8 || tag == 16 || tag == 19 || tag == 20) left[i] = input.readUnsignedShort();
            else if (tag == 9 || tag == 10 || tag == 11 || tag == 12 || tag == 18 || tag == 17) {
                left[i] = input.readUnsignedShort(); right[i] = input.readUnsignedShort();
            } else if (tag == 3 || tag == 4) input.readInt();
            else if (tag == 5 || tag == 6) { input.readLong(); i++; }
            else if (tag == 15) { input.readUnsignedByte(); input.readUnsignedShort(); }
            else throw gap("unsupported macro classfile constant " + tag);
        }
        int poolEnd = original.length - input.available();
        int universeRef = 0, globalClass = 0, analyzerRef = 0, resetRef = 0, messageRef = 0;
        for (int i = 1; i < count; i++) {
            if ("scala/tools/nsc/Global".equals(text[left[i]]) && right[i] == 0) globalClass = i;
            if (right[i] == 0 || left[i] == 0) continue;
            String owner = text[left[left[i]]];
            String name = text[left[right[i]]];
            String descriptor = text[right[right[i]]];
            if ("shapeless/package$".equals(owner) && "implicitNotFoundMessage".equals(name)
                    && "(Lscala/reflect/macros/whitebox/Context;Lscala/reflect/api/Types$TypeApi;)Ljava/lang/String;".equals(descriptor)) messageRef = i;
            if ("scala/reflect/macros/whitebox/Context".equals(owner) && "universe".equals(name)) universeRef = i;
            if ("scala/tools/nsc/Global".equals(owner) && "analyzer".equals(name)) analyzerRef = i;
            if ("scala/tools/nsc/typechecker/Analyzer".equals(owner) && "resetImplicits".equals(name)) resetRef = i;
        }
        if (diagnostic ? messageRef == 0 : universeRef == 0 || globalClass == 0 || analyzerRef == 0 || resetRef == 0)
            throw gap("macro bytecode does not match the supported compiler API");
        byte[] patched = original.clone();
        byte[] pattern = diagnostic
            ? new byte[] {(byte)0xb6, (byte)(messageRef >> 8), (byte)messageRef}
            : new byte[] {(byte)0xb9, (byte)(universeRef >> 8), (byte)universeRef, 1, 0,
            (byte)0xc0, (byte)(globalClass >> 8), (byte)globalClass,
            (byte)0xb6, (byte)(analyzerRef >> 8), (byte)analyzerRef,
            (byte)0xb9, (byte)(resetRef >> 8), (byte)resetRef, 1, 0};
        int changed = 0;
        // Inspect only Code attributes. All constant-pool indices and code
        // lengths stay unchanged; a bridge plus NOPs preserves stack maps.
        input.skipBytes(6);
        input.skipBytes(input.readUnsignedShort() * 2);
        for (int section = 0; section < 2; section++) {
            int members = input.readUnsignedShort();
            for (int member = 0; member < members; member++) {
                input.skipBytes(6);
                int attributes = input.readUnsignedShort();
                for (int attribute = 0; attribute < attributes; attribute++) {
                    String attributeName = text[input.readUnsignedShort()];
                    int length = input.readInt();
                    int start = original.length - input.available();
                    if (section == 1 && "Code".equals(attributeName)) {
                        java.io.DataInputStream code = new java.io.DataInputStream(
                            new java.io.ByteArrayInputStream(original, start, length));
                        code.skipBytes(4);
                        int codeLength = code.readInt();
                        for (int i = start + 8; i <= start + 8 + codeLength - pattern.length; i++) {
                            boolean matches = true;
                            for (int j = 0; j < pattern.length; j++) if (patched[i + j] != pattern[j]) { matches = false; break; }
                            if (!matches) continue;
                            patched[i] = (byte)0xb8;
                            patched[i + 1] = (byte)((count + 5) >> 8);
                            patched[i + 2] = (byte)(count + 5);
                            java.util.Arrays.fill(patched, i + 3, i + pattern.length, (byte)0);
                            changed++;
                        }
                    }
                    if (length < 0 || input.skipBytes(length) != length)
                        throw gap("truncated macro classfile attribute");
                }
            }
        }
        if (changed != 1) throw gap("expected one private compiler call, found " + changed);
        java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
        java.io.DataOutputStream d = new java.io.DataOutputStream(bytes);
        d.write(patched, 0, 8); d.writeShort(count + 6);
        d.write(patched, 10, poolEnd - 10);
        utf(d, "ScalaRsMacroEngine"); pair(d, 7, count, 0);
        utf(d, diagnostic ? "implicitNotFoundMessage" : "resetImplicitCaches");
        utf(d, diagnostic ? "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/String;" : "(Ljava/lang/Object;)V");
        pair(d, 12, count + 2, count + 3); pair(d, 10, count + 1, count + 4);
        d.write(patched, poolEnd, patched.length - poolEnd); d.flush();
        return bytes.toByteArray();
    }

    /** A Java-8 classfile for the public ABI of scalactic's owner repair. */
    static byte[] ownerRepairBytes() throws java.io.IOException {
        java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
        java.io.DataOutputStream d = new java.io.DataOutputStream(bytes);
        d.writeInt(0xcafebabe); d.writeShort(0); d.writeShort(52); d.writeShort(13);
        utf(d, "org/scalactic/MacroOwnerRepair"); pair(d, 7, 1, 0);
        utf(d, "java/lang/Object"); pair(d, 7, 3, 0);
        utf(d, "<init>");
        utf(d, "(Lscala/reflect/macros/whitebox/Context;)V");
        utf(d, "Code"); utf(d, "()V"); pair(d, 12, 5, 8); pair(d, 10, 4, 9);
        utf(d, "repairOwners");
        utf(d, "(Lscala/reflect/api/Exprs$Expr;)Lscala/reflect/api/Exprs$Expr;");
        d.writeShort(0x21); d.writeShort(2); d.writeShort(4);
        d.writeShort(0); d.writeShort(0); d.writeShort(2);
        code(d, 5, 6, 7, 1, 2, new byte[]{0x2a,(byte)0xb7,0,10,(byte)0xb1});
        code(d, 11, 12, 7, 1, 2, new byte[]{0x2b,(byte)0xb0});
        d.writeShort(0); d.flush();
        return bytes.toByteArray();
    }

    static final java.util.Map<Class<?>, Class<?>> reflectionBundles = new java.util.HashMap<>();

    /** Keep the original derivation implementation while routing its private
     * compiler helpers to the reflection universe and native call-site typer.
     * No source compilation or replacement materializer runs in this JVM. */
    static Class<?> compatibleBundleClass(Class<?> implementation) throws Exception {
        Class<?> family;
        try { family = loadClass("shapeless.CaseClassMacros"); }
        catch (ClassNotFoundException absent) { return implementation; }
        if (!family.isAssignableFrom(implementation)) return implementation;
        Class<?> known = reflectionBundles.get(implementation);
        if (known != null) return known;
        BundleBytes bytes = new BundleBytes("ScalaRsReflectionBundle" + reflectionBundles.size(), implementation);
        String type = "Lscala/reflect/api/Types$TypeApi;";
        String symbol = "Lscala/reflect/api/Symbols$SymbolApi;";
        String tree = "Lscala/reflect/api/Trees$TreeApi;";
        bytes.bridge("prefix", "(" + type + ")" + type, "reflectionPrefix", 1, type);
        bytes.bridge("patchedCompanionSymbolOf", "(" + symbol + ")" + symbol, "reflectionCompanion", 1, symbol);
        bytes.bridge("companionRef", "(" + type + ")" + tree, "reflectionCompanionRef", 1, tree);
        bytes.bridge("mkAttributedRef", "(" + type + ")" + tree, "reflectionTypeRef", 1, tree);
        bytes.bridge("mkAttributedRef", "(" + type + symbol + ")" + tree, "reflectionRef", 2, tree);
        bytes.bridge("isAccessible", "(" + type + symbol + ")Z", "reflectionAccessible", 2, "Z");
        class BundleLoader extends ClassLoader {
            BundleLoader() { super(implementation.getClassLoader()); }
            Class<?> define(byte[] data) { return defineClass(null, data, 0, data.length); }
        }
        known = new BundleLoader().define(bytes.finish());
        reflectionBundles.put(implementation, known);
        return known;
    }

    public static Object reflectionPrefix(Object type) throws Exception {
        return call(type, "prefix", 0);
    }

    public static Object reflectionCompanion(Object symbol) throws Exception {
        Long id = sourceSymbolIds.get(symbol);
        if (id != null) {
            long companion = Long.parseLong(query("(q companion " + id + ")").items.get(2).text());
            return companion == 0 ? call(universe, "NoSymbol", 0) : sourceSymbol(companion);
        }
        return call(symbol, "companion", 0);
    }

    public static Object reflectionRef(Object prefix, Object symbol) throws Exception {
        return call(call(universe, "gen", 0), "mkAttributedRef", 2, prefix, symbol);
    }

    public static Object reflectionTypeRef(Object type) throws Exception {
        return reflectionRef(reflectionPrefix(type), call(type, "typeSymbol", 0));
    }

    public static Object reflectionCompanionRef(Object type) throws Exception {
        Object symbol = call(type, "typeSymbol", 0);
        Object companion = reflectionCompanion(symbol);
        if (companion == call(universe, "NoSymbol", 0))
            return call(ScalaRsMacroEngine.companion("Ident"), "apply", 1,
                call(call(symbol, "name", 0), "toTermName", 0));
        return reflectionRef(reflectionPrefix(type), companion);
    }

    public static boolean reflectionAccessible(Object prefix, Object symbol) throws Exception {
        Long id = sourceSymbolIds.get(symbol);
        if (id == null) {
            // Public binary declarations have no call-site-dependent boundary.
            if (!Boolean.TRUE.equals(call(symbol, "isPrivate", 0))
                    && !Boolean.TRUE.equals(call(symbol, "isProtected", 0))
                    && call(symbol, "privateWithin", 0) == call(universe, "NoSymbol", 0)) return true;
            StringBuilder owner = new StringBuilder(), pre = new StringBuilder();
            serType(call(call(symbol, "owner", 0), "toType", 0), owner); serType(prefix, pre);
            Sexp answer = query("(q isAccessibleMember " + owner + " "
                + Sexp.quote(String.valueOf(call(symbol, "name", 0))) + " " + pre + ")");
            return "true".equals(answer.items.get(2).text());
        }
        StringBuilder wire = new StringBuilder(); serType(prefix, wire);
        Sexp answer = query("(q isAccessible " + id + " " + wire + ")");
        return "true".equals(answer.items.get(2).text());
    }

    /** Tiny straight-line subclass: constructor plus reflection helper bridges.
     * Its methods have no branches or exception handlers, so no stack maps are
     * required. All inherited macro methods retain their original bytecode. */
    static final class BundleBytes {
        final java.io.ByteArrayOutputStream poolBytes = new java.io.ByteArrayOutputStream();
        final java.io.DataOutputStream pool = new java.io.DataOutputStream(poolBytes);
        final java.io.ByteArrayOutputStream methodBytes = new java.io.ByteArrayOutputStream();
        final java.io.DataOutputStream methods = new java.io.DataOutputStream(methodBytes);
        int entries = 1, methodCount = 0;
        final int thisClass, superClass, codeName;
        BundleBytes(String name, Class<?> parent) throws Exception {
            thisClass = cls(name); superClass = cls(parent.getName().replace('.', '/')); codeName = text("Code");
            String ctor = "(Lscala/reflect/macros/whitebox/Context;)V";
            int init = method(superClass, "<init>", ctor);
            code(methods, text("<init>"), text(ctor), codeName, 2, 2,
                new byte[]{0x2a,0x2b,(byte)0xb7,(byte)(init >> 8),(byte)init,(byte)0xb1});
            methodCount++;
        }
        int text(String value) throws Exception { int id = entries++; utf(pool, value); return id; }
        int cls(String name) throws Exception { int utf = text(name), id = entries++; pair(pool, 7, utf, 0); return id; }
        int method(int owner, String name, String descriptor) throws Exception {
            int n = text(name), d = text(descriptor), nt = entries++; pair(pool, 12, n, d);
            int id = entries++; pair(pool, 10, owner, nt); return id;
        }
        void bridge(String name, String descriptor, String helper, int args, String result) throws Exception {
            String erased = "(" + (args == 2 ? "Ljava/lang/Object;Ljava/lang/Object;" : "Ljava/lang/Object;")
                + ")" + (result.equals("Z") ? "Z" : "Ljava/lang/Object;");
            int target = method(cls("ScalaRsMacroEngine"), helper, erased);
            java.io.ByteArrayOutputStream body = new java.io.ByteArrayOutputStream();
            for (int i = 1; i <= args; i++) body.write(0x2a + i);
            body.write(0xb8); body.write(target >> 8); body.write(target);
            if (result.equals("Z")) body.write(0xac);
            else {
                int cast = cls(result.substring(1, result.length() - 1));
                body.write(0xc0); body.write(cast >> 8); body.write(cast); body.write(0xb0);
            }
            code(methods, text(name), text(descriptor), codeName, args, args + 1, body.toByteArray());
            methodCount++;
        }
        byte[] finish() throws Exception {
            java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
            java.io.DataOutputStream out = new java.io.DataOutputStream(bytes);
            out.writeInt(0xcafebabe); out.writeShort(0); out.writeShort(52); out.writeShort(entries);
            out.write(poolBytes.toByteArray()); out.writeShort(0x21); out.writeShort(thisClass); out.writeShort(superClass);
            out.writeShort(0); out.writeShort(0); out.writeShort(methodCount); out.write(methodBytes.toByteArray());
            out.writeShort(0); out.flush(); return bytes.toByteArray();
        }
    }

    // ---------------------------------------------------------------- request

    static Object sourceFile(String text, String path) throws Exception {
        Class<?> virtual = loadClass("scala.reflect.io.VirtualFile");
        Object file = virtual.getConstructor(String.class, String.class)
            .newInstance(new java.io.File(path).getName(), path);
        Class<?> abstractFile = loadClass("scala.reflect.io.AbstractFile");
        return loadClass("scala.reflect.internal.util.BatchSourceFile")
            .getConstructor(abstractFile, char[].class)
            .newInstance(file, text.toCharArray());
    }

    static String handle(String line) throws Exception {
        Sexp req = Sexp.parse(line);
        if (!req.isList() || req.items.isEmpty()) {
            return err("malformed request");
        }
        String head = req.items.get(0).atom;
        if ("quit".equals(head)) {
            System.exit(0);
        }
        if ("timing".equals(head)) {
            String reply = "(timing " + invokeNanos + " " + waitNanos + " " + handleNanos + ")";
            invokeNanos = 0;
            waitNanos = 0;
            handleNanos = 0;
            return reply;
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

        // The file this call sits in. Registered before anything else reads
        // the request: its text arrives only with the first request that
        // needs it, whichever branch below that request takes.
        Sexp position = req.field("position");
        Object source = null;
        int positionSize = position.items.size();
        if (positionSize == 4) {
            source = sourceFile(position.items.get(1).text(), position.items.get(2).text());
        } else if ((positionSize == 5 || positionSize == 6)
                && "src".equals(position.items.get(1).text())) {
            int index = Integer.parseInt(position.items.get(2).text());
            if (positionSize == 6) {
                sourceFiles.put(index, sourceFile(
                    position.items.get(3).text(), position.items.get(4).text()));
            }
            source = sourceFiles.get(index);
            if (source == null) {
                return err("macro position names source " + index + ", which was never sent");
            }
        }
        Class<?> implCls;
        try {
            implCls = loadClass(className);
        } catch (ClassNotFoundException e) {
            return err("macro implementation class " + className
                + " is not on the macro classpath (nsc requires the implementation to have "
                + "been compiled by an earlier run)");
        }
        boolean bundle = "true".equals(req.field("bundle").items.get(1).text());
        Object receiver = bundle ? null : implCls.getField("MODULE$").get(null);
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
        structuralTypes.clear();
        structuralParams.clear();
        asyncMarks.clear();
        Ctx handler = new Ctx();
        // `c.prefix`: the receiver of the macro application, or the reason
        // there is none -- which is raised only if the implementation reads it.
        Sexp pfx = req.field("prefix").items.get(1);
        if (pfx.isList() && "no".equals(pfx.items.get(0).atom)) {
            handler.prefixWhy = pfx.items.get(1).text();
        } else {
            handler.prefixTree = buildTree(pfx);
            Sexp prefixType = req.field("prefixType");
            if (prefixType.items.size() == 2) {
                call(handler.prefixTree, "setType", 1, typeFor(prefixType.items.get(1)));
            }
        }
        // `c.macroApplication`: the call as written, carried the same way.
        Sexp app = req.field("app").items.get(1);
        if (app.isList() && "no".equals(app.items.get(0).atom)) {
            handler.appWhy = app.items.get(1).text();
        } else {
            handler.appTree = buildTree(app);
            for (Sexp field : req.items) {
                if (field.isList() && field.items.size() == 2
                        && "appSymbol".equals(field.items.get(0).text())) {
                    Object reference = handler.appTree;
                    while ("Apply".equals(call(reference, "productPrefix", 0))
                            || "TypeApply".equals(call(reference, "productPrefix", 0))) {
                        reference = call(reference, "fun", 0);
                    }
                    call(reference, "setSymbol", 1,
                        sourceSymbol(Long.parseLong(field.items.get(1).text())));
                }
            }
            Sexp appType = req.field("appType");
            if (appType.items.size() == 2) call(handler.appTree, "setType", 1, typeFor(appType.items.get(1)));
            if (source != null) {
                Class<?> sourceClass = loadClass("scala.reflect.internal.util.SourceFile");
                Object pos = loadClass("scala.reflect.internal.util.OffsetPosition")
                    .getConstructor(sourceClass, int.class)
                    .newInstance(source, Integer.parseInt(position.items.get(positionSize - 1).text()));
                call(handler.appTree, "setPos", 1, pos);
            }
        }

        // `scala.reflect.macros.whitebox.Context` *extends* the blackbox one,
        // so a single proxy serves both kinds of implementation. Declaring only
        // the blackbox interface made every whitebox implementation an
        // `IllegalArgumentException: argument type mismatch` from
        // `Method.invoke`, which is not a diagnostic. The handler also answers
        // whitebox-specific context members through this proxy.
        Object ctx = Proxy.newProxyInstance(
            ScalaRsMacroEngine.class.getClassLoader(),
            new Class<?>[]{loadClass("scala.reflect.macros.whitebox.Context")},
            handler);
        List<Long> contextIds = new ArrayList<>();
        for (Sexp field : req.items) {
            if (field.isList() && !field.items.isEmpty()
                    && "contexts".equals(field.items.get(0).text())) {
                for (int i = 1; i < field.items.size(); i++) {
                    contextIds.add(Long.parseLong(field.items.get(i).text()));
                }
            }
        }
        // The Rust typer keeps an outer context open while typing its result,
        // even after its implementation returned. Reuse that exact proxy so
        // outer prefixes, applications and context identity remain available.
        macroContexts.keySet().retainAll(contextIds);
        openMacroContexts = new ArrayList<>();
        if (contextIds.isEmpty()) {
            // Standalone protocol clients predating context identities.
            openMacroContexts.add(ctx);
        } else {
            macroContexts.put(contextIds.get(0), ctx);
            for (Long id : contextIds) {
                Object active = macroContexts.get(id);
                if (active == null) throw gap("active macro context " + id + " is unavailable");
                openMacroContexts.add(active);
            }
        }
        // 2.11 onwards an implementation may take a raw `c.Tree` instead of a
        // `c.Expr[T]`, and slick's `mapToImpl` does. Which one is wanted is
        // read off the implementation's *source* signature by scala-rs and
        // sent along, because the erased signature does not always say: an
        // abstract type member erases to `Object` in class files scala-rs
        // itself writes. Handing an `Expr` to a `Tree` parameter is an
        // `IllegalArgumentException` from `Method.invoke`, not a diagnostic.
        List<Object> argv = new ArrayList<>();
        if (!bundle) argv.add(ctx);
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
        long invokeStarted = System.nanoTime();
        try {
            if (bundle) {
                java.lang.reflect.Constructor<?> constructor = null;
                Class<?> bundleClass = compatibleBundleClass(implCls);
                for (java.lang.reflect.Constructor<?> c : bundleClass.getConstructors()) {
                    if (c.getParameterCount() == 1 && c.getParameterTypes()[0].isInstance(ctx)) {
                        if (constructor != null) return err("ambiguous Context constructor for macro bundle " + className);
                        constructor = c;
                    }
                }
                if (constructor == null) return err("no Context constructor for macro bundle " + className);
                receiver = constructor.newInstance(ctx);
            }
            result = impl.invoke(receiver, argv.toArray());
        } catch (InvocationTargetException e) {
            invokeNanos += System.nanoTime() - invokeStarted;
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
        invokeNanos += System.nanoTime() - invokeStarted;
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
        Class<?> exprCls = loadClass("scala.reflect.api.Exprs$Expr");
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
     * This is the reverse direction of the bridge (`docs/macros.md` §7.20).
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
    /** Per-expansion transport state must survive a nested implementation.
     * Symbol identities and the fresh-name supply remain shared for the run. */
    static final class ExpansionFrame {
        final java.util.IdentityHashMap<Object, Long> trees = new java.util.IdentityHashMap<>(origTrees);
        final java.util.IdentityHashMap<Object, String> types = new java.util.IdentityHashMap<>(structuralTypes);
        final java.util.HashMap<Long, Object> params = new java.util.HashMap<>(structuralParams);
        final java.util.IdentityHashMap<Object, Object[]> async = new java.util.IdentityHashMap<>(asyncMarks);
        final java.util.HashMap<String, Object> terms = new java.util.HashMap<>(transportedTermSymbols);
        final java.util.HashSet<String> ambiguousTerms = new java.util.HashSet<>(ambiguousTransportedTerms);
        final List<Object> contexts = openMacroContexts;
        final List<String> settings = compilerSettings;
        final String gap = pendingGap;
        final String failure = pendingTypecheckFailure;
        void restore() {
            java.util.HashMap<String, Object> nestedTerms =
                new java.util.HashMap<>(transportedTermSymbols);
            java.util.HashSet<String> nestedAmbiguous =
                new java.util.HashSet<>(ambiguousTransportedTerms);
            origTrees.clear(); origTrees.putAll(trees);
            structuralTypes.clear(); structuralTypes.putAll(types);
            structuralParams.clear(); structuralParams.putAll(params);
            asyncMarks.clear(); asyncMarks.putAll(async);
            transportedTermSymbols.clear(); transportedTermSymbols.putAll(terms);
            ambiguousTransportedTerms.clear(); ambiguousTransportedTerms.addAll(ambiguousTerms);
            for (String name : nestedAmbiguous) {
                transportedTermSymbols.remove(name);
                ambiguousTransportedTerms.add(name);
            }
            for (java.util.Map.Entry<String, Object> entry : nestedTerms.entrySet()) {
                rememberTransportedTerm(entry.getKey(), entry.getValue());
            }
            openMacroContexts = contexts;
            compilerSettings = settings;
            pendingGap = gap;
            pendingTypecheckFailure = failure;
        }
    }

    static Sexp query(String q) throws Exception {
        out.println(q);
        for (;;) {
            long waitStarted = System.nanoTime();
            String line = readWireLine(in);
            waitNanos += System.nanoTime() - waitStarted;
            if (line == null) {
                throw new Gap("scala-rs closed the pipe while the macro was asking it a question");
            }
            Sexp ans = Sexp.parse(line);
            String head = ans.isList() && !ans.items.isEmpty() ? ans.items.get(0).atom : "";
            if ("expand".equals(head) || "timing".equals(head)) {
                ExpansionFrame frame = new ExpansionFrame();
                String reply;
                long started = System.nanoTime();
                try {
                    reply = handle(line);
                } catch (Throwable t) {
                    reply = err(describe(t));
                } finally {
                    frame.restore();
                }
                handleNanos += System.nanoTime() - started;
                out.println(reply);
                continue;
            }
            if ("no".equals(head)) {
                if (ans.items.size() != 2 || ans.items.get(1).isList()) {
                    throw new Gap("scala-rs returned a malformed refusal to a macro query");
                }
                throw gap(ans.items.get(1).text());
            }
            return ans;
        }
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
        String verdict = validateTypecheckAnswer(ans);
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
        // A typed constructor's result also types its New/type-tree nodes.
        // Annotation reflection reads that inner type after a Transformer
        // copies the constructor tree, rather than consulting Apply.tpe.
        Object constructor = built;
        while ("Apply".equals(String.valueOf(call(constructor, "productPrefix", 0))))
            constructor = call(constructor, "fun", 0);
        if ("Select".equals(String.valueOf(call(constructor, "productPrefix", 0)))
                && "<init>".equals(String.valueOf(call(constructor, "name", 0)))) {
            Object target = call(constructor, "qualifier", 0);
            if ("New".equals(String.valueOf(call(target, "productPrefix", 0)))) {
                call(support, "setType", 2, call(target, "tpt", 0), tpe);
                call(support, "setType", 2, target, tpe);
            }
        }
        return call(support, "setType", 2, built, tpe);
    }

    /** Exact response grammar for `c.typecheck`; never index a partial reply. */
    static String validateTypecheckAnswer(Sexp ans) {
        if (!ans.isList() || ans.items.size() < 2 || !"a".equals(ans.items.get(0).atom)
                || ans.items.get(1).isList()) {
            throw gap("scala-rs returned a malformed c.typecheck answer");
        }
        String verdict = ans.items.get(1).atom;
        if ("fail".equals(verdict)) {
            if (ans.items.size() != 3 || ans.items.get(2).isList()) {
                throw gap("scala-rs returned a malformed c.typecheck `fail` answer");
            }
            return verdict;
        }
        if (!"ok".equals(verdict) || ans.items.size() != 4
                || !ans.items.get(2).isList() || !ans.items.get(3).isList()) {
            throw gap("scala-rs returned a malformed c.typecheck `ok` answer");
        }
        return verdict;
    }

    /** `c.inferImplicitValue`: the implicit scope lives in scala-rs. */
    static Object inferImplicitValue(
            Object pt, boolean silent, boolean noMacros, Object pos, Object enclosing)
            throws Exception {
        if (!java.util.Objects.equals(pos, enclosing)) {
            throw gap("scala-rs does not implement c.inferImplicitValue with a non-default `pos`");
        }
        StringBuilder sb = new StringBuilder("(q inferImplicitValue ");
        serType(pt, sb);
        sb.append(' ').append(silent ? "1" : "0")
          .append(' ').append(noMacros ? "1" : "0").append(')');
        Sexp ans = query(sb.toString());
        if (!ans.isList() || ans.items.size() < 2 || !"a".equals(ans.items.get(0).atom)
                || ans.items.get(1).isList()) {
            throw gap("scala-rs returned a malformed c.inferImplicitValue answer");
        }
        String verdict = ans.items.get(1).atom;
        if ("none".equals(verdict)) {
            if (ans.items.size() != 2) {
                throw gap("scala-rs returned a malformed c.inferImplicitValue `none` answer");
            }
            return call(universe, "EmptyTree", 0);
        }
        if (!"ok".equals(verdict) || ans.items.size() != 4) {
            throw gap("scala-rs returned a malformed c.inferImplicitValue answer");
        }
        Sexp typeAnswer = ans.items.get(2);
        if (!typeAnswer.isList() || typeAnswer.items.isEmpty()
                || typeAnswer.items.get(0).isList()) {
            throw gap("scala-rs returned a malformed c.inferImplicitValue type answer");
        }
        String typeTag = typeAnswer.items.get(0).atom;
        boolean same = typeAnswer.isList() && typeAnswer.items.size() == 1
            && "same".equals(typeTag);
        if ("same".equals(typeTag) && !same) {
            throw gap("scala-rs returned a malformed c.inferImplicitValue `same` type");
        }
        if (!same && !java.util.Arrays.asList("ty", "src", "cst", "jclass", "mod",
                "repeated", "byname", "refined", "param", "annot").contains(typeTag)) {
            throw gap("scala-rs returned unknown c.inferImplicitValue type tag `"
                + typeTag + "`");
        }
        Object tpe = same ? pt : typeFor(typeAnswer);
        Object built = buildTree(ans.items.get(3));
        Object support = call(call(universe, "internal", 0), "reificationSupport", 0);
        return call(support, "setType", 2, built, tpe);
    }

    /** `scala.reflect.macros.TypecheckException(NoPosition, msg)`. */
    static Throwable newTypecheckException(String msg) throws Exception {
        Class<?> cls = loadClass("scala.reflect.macros.TypecheckException");
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
    static final java.util.Map<String, Object> transportedTermSymbols = new java.util.HashMap<>();
    static final java.util.Set<String> ambiguousTransportedTerms = new java.util.HashSet<>();

    /** A tree the request describes, built in the runtime universe. */
    static Object buildTree(Sexp s) throws Exception {
        if (s.isList() && s.items.size() == 3 && "orig".equals(s.items.get(0).atom)) {
            Object tree = buildTree(s.items.get(2));
            origTrees.put(tree, Long.parseLong(s.items.get(1).text()));
            return tree;
        }
        Object symbol = null;
        String kind = null;
        boolean transportedTerm = false;
        if (s.isList() && s.items.size() >= 3 && "t".equals(s.items.get(0).atom)) {
            kind = s.items.get(1).text();
            Sexp meta = s.items.get(2);
            if (meta.isList() && !meta.items.isEmpty()) {
                if ("fn".equals(meta.items.get(0).atom)) {
                    Sexp answer = query("(q functionSymbol " + meta.items.get(1).text() + ")");
                    symbol = sourceSymbol(Long.parseLong(answer.items.get(2).text()));
                } else if ("sr".equals(meta.items.get(0).atom)
                        || "srm".equals(meta.items.get(0).atom)) {
                    transportedTerm = "srm".equals(meta.items.get(0).atom);
                    long id = Long.parseLong(meta.items.get(1).text());
                    if ("ValDef".equals(kind) || "DefDef".equals(kind) || "ClassDef".equals(kind)
                            || "Function".equals(kind)) symbol = sourceSymbol(id);
                    else {
                        // References in a macro argument carry the source-run
                        // identity of their definition.  Definitions and
                        // functions are materialised above, but an Ident or
                        // Select can reach this method before its definition
                        // was otherwise needed by the mirror (most notably a
                        // local val selected inside ScalaTest's BooleanMacro).
                        // Leaving the symbol null loses the lexical owner and
                        // makes the reflect macro inspect an error tree. `sr`
                        // carries a source-run identity, while `srm` explicitly
                        // gives a stable classpath member a transport identity.
                        // Other classpath references retain `(s0)`.
                        symbol = sourceSymbol(id);
                    }
                }
            }
        }
        Object tree = buildTreeBody(s);
        if (symbol != null && Boolean.TRUE.equals(call(tree, "hasSymbolField", 0))) {
            call(tree, "setSymbol", 1, symbol);
            if (transportedTerm
                    && ("Ident".equals(kind) || "Select".equals(kind))) {
                String name = String.valueOf(call(tree, "name", 0));
                rememberTransportedTerm(name, symbol);
            }
        }
        return tree;
    }

    static void rememberTransportedTerm(String name, Object symbol) {
        Object prior = transportedTermSymbols.get(name);
        if (prior == null && !ambiguousTransportedTerms.contains(name)) {
            transportedTermSymbols.put(name, symbol);
        } else if (prior != symbol) {
            transportedTermSymbols.remove(name);
            ambiguousTransportedTerms.add(name);
        }
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
            case "SelectFromTypeTree":
                return call(companion("SelectFromTypeTree"), "apply", 2,
                    buildTree(kids.get(0)), typeName(nameOf(kids.get(1))));
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
            case "Import": {
                List<Object> selectors = new ArrayList<>();
                for (Sexp selector : kids.get(1).items.subList(1, kids.get(1).items.size())) {
                    Object rename = nameOf(selector.items.get(2)).isEmpty() ? null : buildName(selector.items.get(2));
                    selectors.add(call(companion("ImportSelector"), "apply", 4,
                        buildName(selector.items.get(1)), -1, rename, -1));
                }
                return call(companion("Import"), "apply", 2, buildTree(kids.get(0)), list(selectors));
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
            case "Match": {
                List<Object> cases = new ArrayList<>();
                for (Sexp k : kids.get(1).items.subList(1, kids.get(1).items.size())) {
                    cases.add(buildTree(k));
                }
                return call(companion("Match"), "apply", 2,
                    buildTree(kids.get(0)), list(cases));
            }
            case "Try":
                return call(companion(kind), "apply", 3, buildTree(kids.get(0)), buildTrees(kids.get(1)), buildTree(kids.get(2)));
            case "CaseDef":
                return call(companion("CaseDef"), "apply", 3,
                    buildTree(kids.get(0)), buildTree(kids.get(1)), buildTree(kids.get(2)));
            case "Bind":
                return call(companion("Bind"), "apply", 2,
                    buildName(kids.get(0)), buildTree(kids.get(1)));
            case "Alternative":
                return call(companion("Alternative"), "apply", 1, buildTrees(kids.get(0)));
            case "UnApply":
                return call(companion("UnApply"), "apply", 2,
                    buildTree(kids.get(0)), buildTrees(kids.get(1)));
            case "Star":
                return call(companion("Star"), "apply", 1, buildTree(kids.get(0)));
            case "TypeTree": {
                Object tree = call(companion("TypeTree"), "apply", 0);
                if (!kids.isEmpty() && !("ty".equals(kids.get(0).items.get(0).atom)
                        && "".equals(kids.get(0).items.get(1).text()))) {
                    Object support = call(call(universe, "internal", 0), "reificationSupport", 0);
                    call(support, "setType", 2, tree, typeFor(kids.get(0)));
                }
                return tree;
            }
            case "Super":
                return call(companion(kind), "apply", 2, buildTree(kids.get(0)), buildName(kids.get(1)));
            case "Return":
            case "Throw":
            case "SingletonTypeTree":
            case "CompoundTypeTree":
            case "New":
                return call(companion(kind), "apply", 1, buildTree(kids.get(0)));
            case "Typed":
            case "TypeBoundsTree":
            case "Assign":
            case "Annotated":
                return call(companion(kind), "apply", 2,
                    buildTree(kids.get(0)), buildTree(kids.get(1)));
            case "TypeDef":
                return call(companion(kind), "apply", 4, buildMods(kids.get(0)),
                    buildName(kids.get(1)), buildTrees(kids.get(2)), buildTree(kids.get(3)));
            case "ExistentialTypeTree":
                return call(companion(kind), "apply", 2,
                    buildTree(kids.get(0)), buildTrees(kids.get(1)));
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
            case "LabelDef":
                return call(companion(kind), "apply", 3, buildName(kids.get(0)), buildTrees(kids.get(1)), buildTree(kids.get(2)));
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
     * classpath, applied to its type arguments; `(src <id> <arg>...)` is a
     * source class or weak type parameter, with no class file for the mirror to
     * find, built by {@link #sourceSymbol} with its info asked for only when
     * forced; `(mod "a.b.Obj")` is a binary object's singleton;
     * `(annot "A" <type>)` is `<type> @A`; `(cst (c "Int" "1"))` is
     * the constant type nsc gives a literal, which `c.typecheck(q"1").tpe`
     * has to be if it is to be the type nsc reports.
     */
    /**
     * {@link #typeForUncached} by the type's wire text.
     *
     * shapeless asks `c.openImplicits` at every `Lazy` it derives, and the
     * answer lists every open implicit with its type: rebuilding each one
     * through reflection, at every level of a deep derivation, was three
     * quarters of the engine's time. A runtime-universe type is immutable,
     * and the same text names the same classes, modules and run symbols, so
     * one answer serves every request. A refinement and a structural type
     * parameter are built with fresh symbols each time and are not kept.
     */
    static final java.util.HashMap<String, Object> typeCache = new java.util.HashMap<>();

    static Object typeFor(Sexp s) throws Exception {
        String key = s.raw();
        if (key != null) {
            Object hit = typeCache.get(key);
            if (hit != null) {
                return hit;
            }
        }
        Object built = typeForUncached(s);
        if (key != null && built != null && !key.contains("(refined") && !key.contains("(param")) {
            typeCache.put(key, built);
        }
        return built;
    }

    static Object typeForUncached(Sexp s) throws Exception {
        String head = s.items.get(0).atom;
        if ("repeated".equals(head)) {
            Object repeated = call(call(universe, "definitions", 0), "RepeatedParamClass", 0);
            List<Object> elements = new ArrayList<>();
            elements.add(typeFor(s.items.get(1)));
            return call(universe, "appliedType", 2, repeated, list(elements));
        }
        if ("byname".equals(head)) {
            Object byName = call(call(universe, "definitions", 0), "ByNameParamClass", 0);
            List<Object> elements = new ArrayList<>();
            elements.add(typeFor(s.items.get(1)));
            return call(universe, "appliedType", 2, byName, list(elements));
        }
        if ("param".equals(head)) {
            Object param = structuralParams.get(Long.parseLong(s.items.get(1).text()));
            if (param == null) throw gap("unbound structural type parameter");
            return ownedTypeRef(call(param, "owner", 0), param);
        }
        if ("refined".equals(head)) {
            Object internal = call(universe, "internal", 0);
            List<Object> parents = new ArrayList<>();
            for (Sexp p : s.items.get(2).items.subList(1, s.items.get(2).items.size())) parents.add(typeFor(p));
            Object scope = call(internal, "newScopeWith", 1, seq(new ArrayList<>()));
            Object result = call(internal, "refinedType", 3, list(parents), call(universe, "NoSymbol", 0), scope);
            Object owner = call(result, "typeSymbol", 0);
            for (Sexp m : s.items.get(3).items.subList(1, s.items.get(3).items.size())) {
                Sexp info = m.items.get(2);
                boolean bounds = "bounds".equals(info.items.get(0).atom);
                Object member = call(internal, "newTypeSymbol", 4, owner, typeName(m.items.get(1).text()), call(universe, "NoPosition", 0), bounds ? flagValue("DEFERRED") : 0L);
                call(member, "setInfo", 1, structuralInfo(member, info));
                call(scope, "enter", 1, member);
            }
            structuralTypes.put(result, s.items.get(1).text());
            return result;
        }
        if ("cst".equals(head)) {
            return call(call(universe, "internal", 0), "constantType", 1,
                constant(s.items.get(1)));
        }
        if ("wild".equals(head)) {
            return call(universe, "WildcardType", 0);
        }
        String name = s.items.get(1).text();
        if ("param".equals(head)) {
            Object owner = call(mirror, "staticClass", 1, name);
            Object parameters = call(owner, "typeParams", 0);
            Object parameter = call(parameters, "apply", 1, Integer.parseInt(s.items.get(2).text()));
            return call(call(parameter, "asType", 0), "toType", 0);
        }
        if ("mod".equals(head)) {
            Object mod = call(mirror, "staticModule", 1, name);
            return call(call(universe, "internal", 0), "singleType", 2,
                call(call(mod, "owner", 0), "thisType", 0), mod);
        }
        if ("src".equals(head)) {
            Object sym = sourceSymbol(Long.parseLong(name));
            List<Object> args = new ArrayList<>();
            for (Sexp arg : s.items.subList(2, s.items.size())) args.add(typeFor(arg));
            Object base = ownedTypeRef(call(sym, "owner", 0), sym);
            return call(call(universe, "internal", 0), "typeRef", 3,
                call(base, "pre", 0), sym, list(args));
        }
        if ("annot".equals(head)) {
            // `T @A` for an annotation class `A` taking no arguments -- the
            // `@uncheckedVariance` nsc puts on a default getter's result.
            Object under = typeFor(s.items.get(2));
            Object annTpe = call(call(call(mirror, "staticClass", 1, name), "asType", 0), "toType", 0);
            Object noJavaArgs = call(loadClass("scala.collection.immutable.ListMap$")
                .getField("MODULE$").get(null), "empty", 0);
            Object ann = call(companion("Annotation"), "apply", 3, annTpe,
                list(new ArrayList<>()), noJavaArgs);
            List<Object> anns = new ArrayList<>();
            anns.add(ann);
            return call(call(universe, "internal", 0), "annotatedType", 2, list(anns), under);
        }
        Object cls = "jclass".equals(head)
            ? call(mirror, "classSymbol", 1, loadClass(name))
            : call(mirror, "staticClass", 1, name);
        if ("jclass".equals(head)) binaryClassNames.put(cls, name);
        if (s.items.size() <= 2) {
            return call(call(cls, "asType", 0), "toTypeConstructor", 0);
        }
        List<Object> args = new ArrayList<>();
        for (Sexp a : s.items.subList(2, s.items.size())) {
            args.add(typeFor(a));
        }
        return call(universe, "appliedType", 2, cls, list(args));
    }

    static final java.util.IdentityHashMap<Object, String> structuralTypes = new java.util.IdentityHashMap<>();
    static final java.util.HashMap<Long, Object> structuralParams = new java.util.HashMap<>();

    static Object structuralInfo(Object owner, Sexp info) throws Exception {
        Object internal = call(universe, "internal", 0);
        String kind = info.items.get(0).atom;
        if ("bounds".equals(kind)) return call(internal, "typeBounds", 2, typeFor(info.items.get(1)), typeFor(info.items.get(2)));
        if (!"poly".equals(kind)) return typeFor(info);
        List<Object> params = new ArrayList<>();
        List<Sexp> defs = info.items.get(1).items.subList(1, info.items.get(1).items.size());
        for (Sexp p : defs) {
            Object param = call(internal, "newTypeSymbol", 4, owner, typeName(p.items.get(1).text()), call(universe, "NoPosition", 0), flagValue("PARAM"));
            structuralParams.put(Long.parseLong(p.items.get(0).text()), param);
            params.add(param);
        }
        for (int i = 0; i < defs.size(); i++) {
            Sexp p = defs.get(i);
            call(params.get(i), "setInfo", 1, call(internal, "typeBounds", 2, typeFor(p.items.get(2)), typeFor(p.items.get(3))));
        }
        return call(internal, "polyType", 2, list(params), typeFor(info.items.get(2)));
    }

    /** `WeakTypeTag` for a type already built in the runtime universe. */
    static Object tagOf(Object tpe) throws Exception {
        Class<?> creatorCls =
            loadClass("scala.reflect.internal.StdCreators$FixedMirrorTypeCreator");
        Object creator = ctor(creatorCls, 3).newInstance(universe, mirror, tpe);
        return call(companion("WeakTypeTag"), "apply", 2, mirror, creator);
    }

    /**
     * Types standing for classes the *calling* compilation run is defining,
     * keyed by their full names, so the same class is always the same symbol
     * and an expansion that mentions it twice mentions one type.
     */

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
            loadClass("scala.reflect.internal.StdCreators$FixedMirrorTreeCreator");
        Object creator = ctor(creatorCls, 3).newInstance(universe, mirror, tree);
        return call(companion("Expr"), "apply", 3, mirror, creator, tag);
    }

    /** Build the small scalar expressions used by ScalaTest's assertion macros. */
    static Object literalExpr(Object value) throws Exception {
        Object tag;
        if (value instanceof Boolean) {
            tag = call(companion("WeakTypeTag"), "Boolean", 0);
        } else if (value instanceof String) {
            Object definitions = call(universe, "definitions", 0);
            Object stringClass = call(definitions, "StringClass", 0);
            Object stringType = call(call(stringClass, "asType", 0), "toType", 0);
            tag = tagOf(stringType);
        } else {
            throw gap("Context.literal only supports String and Boolean for now");
        }
        Object constant = call(companion("Constant"), "apply", 1, value);
        Object tree = call(companion("Literal"), "apply", 1, constant);
        return mkExpr(tree, tag);
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
        if (isA(o, "scala.reflect.api.Trees$ImportSelectorApi")) {
            sb.append("(selector ");
            ser(call(o, "name", 0), sb);
            sb.append(' ');
            Object rename = call(o, "rename", 0);
            if (rename == null) sb.append("(n term \"\")");
            else ser(rename, sb);
            sb.append(')');
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

    static final java.util.IdentityHashMap<Object, Object[]> asyncMarks = new java.util.IdentityHashMap<>();
    static final class AsyncMarker {
        final Object[] data;
        AsyncMarker(Object[] data) { this.data = data; }
    }
    static Object asyncMarkerTag() throws Exception {
        Object companion = loadClass("scala.reflect.ClassTag$").getField("MODULE$").get(null);
        return call(companion, "apply", 1, AsyncMarker.class);
    }

    static Object markAsync(Object[] args) throws Exception {
        if (!compilerSettings.contains("-Xasync"))
            throw gap("-Xasync must be enabled for async transformation");
        Object config = args[3];
        Object keys = call(call(config, "keysIterator", 0), "toList", 0);
        Object it = call(keys, "iterator", 0);
        while ((Boolean) call(it, "hasNext", 0)) {
            String key = String.valueOf(call(it, "next", 0));
            if (key.equals("postAnfTransform") || key.equals("stateDiagram"))
                throw gap("markForAsyncTransform configuration `" + key + "` is not implemented");
        }
        Object await = args[2];
        Object method = call(await, "asMethod", 0);
        Object params = call(call(call(method, "paramLists", 0), "head", 0), "head", 0);
        Object awaitable = call(params, "typeSignature", 0);
        Object typeParams = call(method, "typeParams", 0);
        List<Object> replacements = new ArrayList<>();
        Object tp = call(typeParams, "iterator", 0);
        Object anyRef = call(call(universe, "definitions", 0), "AnyRefTpe", 0);
        while ((Boolean) call(tp, "hasNext", 0)) { call(tp, "next", 0); replacements.add(anyRef); }
        awaitable = call(awaitable, "substituteTypes", 2, typeParams, list(replacements));
        Object[] data = new Object[]{call(await, "fullName", 0), awaitable,
            call(config, "contains", 1, "allowExceptionsToPropagate")};
        asyncMarks.put(args[1], data);
        call(args[1], "updateAttachment", 2, new AsyncMarker(data), asyncMarkerTag());
        return args[1];
    }

    static void serTree(Object t, StringBuilder sb) throws Exception {
        Object[] async = asyncMarks.get(t);
        if (async == null && !asyncMarks.isEmpty()) {
            Object attached = call(call(t, "attachments", 0), "get", 1, asyncMarkerTag());
            if ((Boolean) call(attached, "isDefined", 0)) async = ((AsyncMarker) call(attached, "get", 0)).data;
        }
        if (async != null) {
            sb.append("(t \"AsyncDefDef\" (s0) ");
            serTreeShape(t, sb);
            sb.append(' ').append(Sexp.quote(String.valueOf(async[0]))).append(' ');
            serType(async[1], sb);
            sb.append(Boolean.TRUE.equals(async[2]) ? " 1)" : " 0)");
            return;
        }
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
        Object attributed = call(t, "tpe", 0);
        if (structuralTypes.containsKey(attributed)) {
            sb.append("(t \"Attributed\" (s0) ");
            serTreeShape(t, sb);
            sb.append(' ');
            serType(attributed, sb);
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
        if ("Annotated".equals(prefix) && Boolean.TRUE.equals(call(t, "isTerm", 0)))
            prefix = "AnnotatedExpr";
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
    static void serSym(Object t, StringBuilder sb) throws Exception {
            Object sym = call(t, "symbol", 0);
            Long sourceId = sourceSymbolIds.get(sym);
            if (sourceId != null) {
                sb.append("(sr ").append(sourceId).append(')');
                return;
            }
            if (sym == null || sym == call(universe, "NoSymbol", 0)) {
                String prefix = String.valueOf(call(t, "productPrefix", 0));
                if ("Ident".equals(prefix)) {
                    String name = String.valueOf(call(t, "name", 0));
                    Object transported = transportedTermSymbols.get(name);
                    Long transportedId = sourceSymbolIds.get(transported);
                    if (transportedId != null) {
                        sb.append("(sr ").append(transportedId).append(')');
                        return;
                    }
                }
                // Synthetic type trees in a macro result quite often have no
                // symbol field even though their attributed type retains the
                // exact member symbol.  CompoundTypeTree parents produced by
                // ZIO's autoTrace macro are one such case.
                Object tpe = call(t, "tpe", 0);
                if (tpe != null && tpe != call(universe, "NoType", 0)
                        && isA(tpe, "scala.reflect.internal.Types$TypeRef")) {
                    sym = call(tpe, "typeSymbolDirect", 0);
                }
                if (sym == null || sym == call(universe, "NoSymbol", 0)) {
                    sb.append("(s0)");
                    return;
                }
            }
            sourceId = sourceSymbolIds.get(sym);
            if (sourceId != null) {
                sb.append("(sr ").append(sourceId).append(')');
                return;
            }
            // A package `This` (`p.this.C`) is a path anchor, not an
            // enclosing-class receiver. Keep that distinction in the wire
            // descriptor: a plain static full name does not tell the Rust
            // side whether it names a package or a class.
            if (Boolean.TRUE.equals(call(sym, "isPackageClass", 0))) {
                sb.append("(sp ").append(Sexp.quote(String.valueOf(call(sym, "fullName", 0))))
                  .append(')');
                return;
            }
            // A binary object's attributed This can name an object outside
            // the expansion site. Its stable path is not the caller's this,
            // and a class with a companion must remain distinguishable.
            if ("This".equals(String.valueOf(call(t, "productPrefix", 0)))
                    && Boolean.TRUE.equals(call(sym, "isModuleClass", 0))
                    && (Boolean.TRUE.equals(call(sym, "isStatic", 0))
                        || hasStaticModuleField(sym))) {
                sb.append("(sm ").append(Sexp.quote(String.valueOf(call(sym, "fullName", 0))))
                  .append(')');
                return;
            }
            // Only a *static* symbol survives the trip: scala-rs resolves it
            // by full name, and a local or a parameter has no such name.
            Object isStatic = call(sym, "isStatic", 0);
            if (!Boolean.TRUE.equals(isStatic)) {
                Object owner = call(sym, "owner", 0);
                // Methods of an object are instance methods in bytecode, so
                // reflection does not mark the method itself static. A method
                // owned by a static module class still has a stable root path,
                // including modules nested in companion objects. Preserve its
                // full identity so the call-site typer can rebuild a qualified
                // selection instead of resolving a bare implementation-local
                // name.
                if (Boolean.TRUE.equals(call(owner, "isModuleClass", 0))
                        && (Boolean.TRUE.equals(call(owner, "isStatic", 0))
                            || hasStaticModuleField(owner))) {
                    sb.append("(s ").append(Sexp.quote(String.valueOf(call(sym, "fullName", 0))))
                      .append(')');
                    return;
                }
                // A path-dependent type has no static path of its own, but
                // its declaration does. Preserve that member identity so the
                // call-site typer need not re-resolve the engine's prefix.
                if (Boolean.TRUE.equals(call(sym, "isType", 0))
                        && !Boolean.TRUE.equals(call(sym, "isClass", 0))
                        && Boolean.TRUE.equals(call(owner, "isClass", 0))
                        && staticByOwners(owner)) {
                    sb.append("(tm ")
                      .append(Sexp.quote(String.valueOf(call(owner, "fullName", 0))))
                      .append(' ')
                      .append(Sexp.quote(String.valueOf(call(sym, "name", 0))))
                      .append(')');
                    return;
                }
                sb.append("(s0)");
                return;
            }
            sb.append("(s ").append(Sexp.quote(String.valueOf(call(sym, "fullName", 0))))
              .append(')');
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
        if (structuralTypes.containsKey(tpe)) {
            sb.append("(ty ").append(Sexp.quote(structuralTypes.get(tpe))).append(')');
            return;
        }
        if (tpe == null || tpe == call(universe, "NoType", 0)) {
            sb.append("(ty \"\")");
            return;
        }
        if (tpe == call(universe, "WildcardType", 0)) {
            sb.append("(wild)");
            return;
        }
        if (isA(tpe, "scala.reflect.internal.Types$ConstantType")) {
            sb.append("(cst ");
            serConstant(call(tpe, "value", 0), sb);
            sb.append(')');
            return;
        }
        if (isA(tpe, "scala.reflect.internal.Types$RefinedType")
                && Boolean.TRUE.equals(call(call(tpe, "decls", 0), "isEmpty", 0))) {
            sb.append("(intersection");
            Object parents = call(call(tpe, "parents", 0), "iterator", 0);
            while ((Boolean) call(parents, "hasNext", 0)) {
                sb.append(' ');
                serType(call(parents, "next", 0), sb);
            }
            sb.append(')');
            return;
        }
        if (isA(tpe, "scala.reflect.internal.Types$ThisType")) {
            Object owner = call(tpe, "typeSymbol", 0);
            if (Boolean.TRUE.equals(call(owner, "isModuleClass", 0))) {
                Object module = call(owner, "sourceModule", 0);
                if (staticByOwners(module)) {
                    sb.append("(mod ").append(Sexp.quote(String.valueOf(call(module, "fullName", 0)))).append(')');
                    return;
                }
            }
        }
        if (isA(tpe, "scala.reflect.internal.Types$SingleType")) {
            Object term = call(tpe, "termSymbol", 0);
            if (Boolean.TRUE.equals(call(term, "isModule", 0)) && staticByOwners(term)) {
                sb.append("(mod ").append(Sexp.quote(String.valueOf(call(term, "fullName", 0)))).append(')');
                return;
            }
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
        if (sym == call(call(universe, "definitions", 0), "RepeatedParamClass", 0)) {
            sb.append("(repeated ");
            serType(call(call(d, "typeArgs", 0), "head", 0), sb);
            sb.append(')');
            return;
        }
        if (sym == call(call(universe, "definitions", 0), "ByNameParamClass", 0)) {
            sb.append("(byname ");
            serType(call(call(d, "typeArgs", 0), "head", 0), sb);
            sb.append(')');
            return;
        }
        String binaryName = binaryClassNames.get(sym);
        if (binaryName != null && Boolean.TRUE.equals(call(sym, "isClass", 0))) {
            Object constructor = call(call(sym, "asType", 0), "toTypeConstructor", 0);
            // Retain only the projection originally transported by JVM
            // identity. A concrete outer-instance prefix is a different type.
            if (call(d, "pre", 0).equals(call(constructor, "pre", 0))) {
                sb.append("(jclass ").append(Sexp.quote(binaryName));
                Object it = call(call(d, "typeArgs", 0), "iterator", 0);
                while ((Boolean) call(it, "hasNext", 0)) {
                    sb.append(' ');
                    serType(call(it, "next", 0), sb);
                }
                sb.append(')');
                return;
            }
        }
        if (sourceSymbolIds.containsKey(sym) && (Boolean.TRUE.equals(call(sym, "isClass", 0))
                || Boolean.TRUE.equals(call(sym, "isTypeParameter", 0))
                || Boolean.TRUE.equals(call(sym, "isAliasType", 0)))) {
            // Return source identities directly: names cannot distinguish
            // nested classes or type parameters belonging to different owners.
            sb.append("(src ").append(sourceSymbolIds.get(sym));
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
            if (isA(d, "scala.reflect.internal.Types$RefinedType")
                    && Boolean.TRUE.equals(call(call(d, "decls", 0), "isEmpty", 0))) {
                serType(d, sb);
                return;
            }
            if (!isA(d, "scala.reflect.internal.Types$TypeRef")) {
                sb.append("(tyx ").append(Sexp.quote(String.valueOf(tpe))).append(')');
                return;
            }
            sym = call(d, "typeSymbolDirect", 0);
        }
        if (!Boolean.TRUE.equals(call(sym, "isClass", 0))) {
            Object owner = call(sym, "owner", 0);
            if (Boolean.TRUE.equals(call(owner, "isClass", 0)) && staticByOwners(owner)) {
                Object pre = call(d, "pre", 0);
                Object term = call(pre, "termSymbol", 0);
                Object noPrefix = call(universe, "NoPrefix", 0);
                Object termOwner = term == null || term == call(universe, "NoSymbol", 0)
                    ? null : call(term, "owner", 0);
                boolean noPrefixType = pre == noPrefix;
                boolean staticTermPrefix = termOwner != null
                    && Boolean.TRUE.equals(call(termOwner, "isClass", 0))
                    && staticByOwners(termOwner);
                if (!noPrefixType && !staticTermPrefix) {
                    // A local/parameter/instance prefix is part of the type's
                    // identity. Sending only the declaration would turn p.T
                    // into every other q.T from the same owner.
                    sb.append("(tyx ").append(Sexp.quote(String.valueOf(tpe))).append(')');
                    return;
                }
                sb.append("(mem ")
                  .append(Sexp.quote(String.valueOf(call(owner, "fullName", 0))))
                  .append(' ')
                  .append(Sexp.quote(String.valueOf(call(sym, "name", 0))));
                // A path-dependent member is identified by both its
                // declaration and the stable value prefix.  `a.Type` and
                // `b.Type` share the declaration but are distinct types.
                if (staticTermPrefix) {
                    sb.append(" (pre ")
                      .append(Sexp.quote(String.valueOf(call(termOwner, "fullName", 0))))
                      .append(' ')
                      .append(Sexp.quote(String.valueOf(call(term, "name", 0))))
                      .append(')');
                }
                Object args = call(d, "typeArgs", 0);
                Object it = call(args, "iterator", 0);
                while ((Boolean) call(it, "hasNext", 0)) {
                    sb.append(' ');
                    serType(call(it, "next", 0), sb);
                }
                sb.append(')');
                return;
            }
        }
        if (Boolean.TRUE.equals(call(sym, "isModuleClass", 0)) && staticByOwners(sym)) {
            Object module = call(sym, "sourceModule", 0);
            if (module != null && module != call(universe, "NoSymbol", 0)
                    && staticByOwners(module)) {
                sb.append("(mod ")
                  .append(Sexp.quote(String.valueOf(call(module, "fullName", 0))))
                  .append(')');
                return;
            }
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

    /** True when a module class is represented by a JVM singleton field. */
    static boolean hasStaticModuleField(Object moduleClass) {
        try {
            Object classSymbol = call(moduleClass, "asClass", 0);
            Class<?> runtimeClass = (Class<?>) call(mirror, "runtimeClass", 1, classSymbol);
            return Modifier.isStatic(runtimeClass.getField("MODULE$").getModifiers());
        } catch (Throwable unavailable) {
            return false;
        }
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

    static boolean binaryParametersMatch(Object method, Sexp wanted) throws Exception {
        Object clauses = call(call(method, "paramLists", 0), "iterator", 0);
        int slot = 1;
        while ((Boolean) call(clauses, "hasNext", 0)) {
            Object params = call(call(clauses, "next", 0), "iterator", 0);
            while ((Boolean) call(params, "hasNext", 0)) {
                Object param = call(params, "next", 0);
                if (slot >= wanted.items.size()) return false;
                String expected = wanted.items.get(slot++).text();
                if (expected.isEmpty()) continue;
                String descriptor;
                Object tpe = call(param, "typeSignature", 0);
                if (Boolean.TRUE.equals(call(param, "isByNameParam", 0))) {
                    descriptor = "Lscala/Function0;";
                } else if ("scala.<repeated>".equals(String.valueOf(call(call(tpe, "typeSymbol", 0), "fullName", 0)))) {
                    descriptor = "Lscala/collection/immutable/Seq;";
                } else {
                    Class<?> cls = (Class<?>) call(mirror, "runtimeClass", 1, call(tpe, "erasure", 0));
                    if (cls == void.class) descriptor = "Lscala/runtime/BoxedUnit;";
                    else if (cls == boolean.class) descriptor = "Z";
                    else if (cls == byte.class) descriptor = "B";
                    else if (cls == short.class) descriptor = "S";
                    else if (cls == char.class) descriptor = "C";
                    else if (cls == int.class) descriptor = "I";
                    else if (cls == long.class) descriptor = "J";
                    else if (cls == float.class) descriptor = "F";
                    else if (cls == double.class) descriptor = "D";
                    else descriptor = cls.isArray() ? cls.getName().replace('.', '/')
                        : "L" + cls.getName().replace('.', '/') + ";";
                }
                if (!expected.equals(descriptor)) return false;
            }
        }
        return slot == wanted.items.size();
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
        String binaryName = answer.items.size() > 8 ? answer.items.get(8).text() : "";
        if (!binaryName.isEmpty()) {
            try {
                // The runtime mirror prints some rejected Scala signatures to
                // stderr before throwing them.  A synthetic mirror symbol is
                // the supported fallback below, so keep that recoverable
                // probe from leaking a full stack trace from a successful
                // compilation.
                java.io.PrintStream oldErr = System.err;
                Object cls;
                try {
                    System.setErr(new java.io.PrintStream(new java.io.ByteArrayOutputStream()));
                    cls = call(mirror, "classSymbol", 1, loadClass(binaryName.replace('/', '.')));
                } finally {
                    System.setErr(oldErr);
                }
                Object symbol = "Module".equals(kind) ? call(cls, "sourceModule", 0) : cls;
                sourceSymbols.put(id, symbol);
                sourceSymbolIds.put(symbol, id);
                binarySymbols.add(symbol);
                binaryClassNames.put(cls, binaryName.replace('/', '.'));
                return symbol;
            } catch (Throwable unreadableBinarySymbol) {
                // Runtime reflection can reject an otherwise valid Scala 2
                // package signature while opening the owner of this class.
                // The compiler universe used by scalac accepts that same
                // signature. Mirror the symbol through the reverse RPC below
                // instead; its lazy info still comes from scala-rs and gives
                // macros the source-level type without forcing the runtime
                // mirror to unpickle the unrelated package object.
            }
        }
        Object owner = parent == 0 ? call(mirror, "EmptyPackageClass", 0) : sourceSymbol(parent);
        known = sourceSymbols.get(id);
        if (known != null) return known;
        if (!binaryName.isEmpty() && Boolean.TRUE.equals(call(owner, "isPackageClass", 0))) {
            Object lookupName = "ModuleClass".equals(kind) || "Module".equals(kind)
                ? termName(name.endsWith("$") ? name.substring(0, name.length() - 1) : name)
                : typeName(name);
            Object existing = call(call(owner, "info", 0), "decl", 1, lookupName);
            if (existing != call(universe, "NoSymbol", 0)) {
                Object symbol = "ModuleClass".equals(kind) ? call(existing, "moduleClass", 0) : existing;
                sourceSymbols.put(id, symbol);
                sourceSymbolIds.put(symbol, id);
                return symbol;
            }
        }
        if (binarySymbols.contains(owner)) {
            Object memberName = "TypeMember".equals(kind) || "TypeParam".equals(kind)
                ? typeName(name) : termName(name);
            Object member = call(call(owner, "info", 0), "decl", 1, memberName);
            if (Boolean.TRUE.equals(call(member, "isOverloaded", 0))) {
                Sexp shape = answer.items.get(9);
                Object alternatives = call(call(member, "alternatives", 0), "iterator", 0);
                Object selected = null;
                while ((Boolean) call(alternatives, "hasNext", 0)) {
                    Object candidate = call(alternatives, "next", 0);
                    if (!Boolean.TRUE.equals(call(candidate, "isMethod", 0))) continue;
                    if (((Integer) call(call(candidate, "typeParams", 0), "size", 0))
                            != Integer.parseInt(shape.items.get(1).text())) continue;
                    Object clauses = call(call(candidate, "paramLists", 0), "iterator", 0);
                    int clause = 2;
                    boolean matches = true;
                    while ((Boolean) call(clauses, "hasNext", 0)) {
                        int size = (Integer) call(call(clauses, "next", 0), "size", 0);
                        if (clause >= shape.items.size() || size != Integer.parseInt(shape.items.get(clause++).text()))
                            matches = false;
                    }
                    if (!matches || clause != shape.items.size()) continue;
                    if (!binaryParametersMatch(candidate, answer.items.get(10))) continue;
                    if (selected != null) throw gap("binary symbol has multiple matching alternatives: " + full);
                    selected = candidate;
                }
                if (selected == null) throw gap("binary symbol has no matching alternative: " + full);
                member = selected;
            }
            if (member != call(universe, "NoSymbol", 0)) {
                sourceSymbols.put(id, member);
                sourceSymbolIds.put(member, id);
                binarySymbols.add(member);
                return member;
            }
        }
        Object internal = call(universe, "internal", 0);
        Object pos = call(universe, "NoPosition", 0);
        long flags = flagsOf(answer.items.get(7));
        if ("ModuleClass".equals(kind) || "Module".equals(kind)) {
            return sourceModule(id, kind, name, full, owner, flags);
        }
        Object symbol;
        if ("Class".equals(kind)) {
            symbol = newSourceSymbol("ClassSymbol", owner, typeName(name), pos, flags);
        } else if ("Method".equals(kind)) {
            symbol = newSourceSymbol("MethodSymbol", owner, termName(name), pos, flags | internalFlag("METHOD"));
        } else if ("Term".equals(kind)) {
            symbol = newSourceSymbol("TermSymbol", owner, termName(name), pos, flags);
        } else if ("TypeMember".equals(kind)) {
            symbol = call(internal, "newTypeSymbol", 4, owner, typeName(name), pos, Long.valueOf(flags));
        } else if ("TypeParam".equals(kind)) {
            symbol = call(internal, "newTypeSymbol", 4, owner, typeName(name), pos,
                Long.valueOf(flags | internalFlag("PARAM") | internalFlag("DEFERRED")));
        } else {
            throw gap("macro mirror cannot describe source symbol kind " + kind);
        }
        sourceSymbols.put(id, symbol);
        sourceSymbolIds.put(symbol, id);
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
                && !Boolean.TRUE.equals(call(symbol, "isTypeParameter", 0))
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
        String adapterKey = kind + ("package".equals(String.valueOf(name)) ? ":package" : "");
        Class<?> adapter = sourceSymbolClasses.get(adapterKey);
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
            sourceSymbolClasses.put(adapterKey, adapter);
        }
        Constructor<?> constructor = adapter.getConstructors()[0];
        // Package-object symbols have a fixed name (`package`), so their
        // runtime constructor takes only the owner and position.
        Object symbol = constructor.getParameterCount() == 2
            ? constructor.newInstance(owner, pos)
            : constructor.newInstance(owner, pos, name);
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
            java.lang.reflect.Field owner = loadClass("scala.reflect.internal.Symbols$Symbol")
                .getDeclaredField("_rawowner");
            owner.setAccessible(true);
            owner.set(symbol, next);
        }
    }

    static long internalFlag(String name) throws Exception {
        Object flags = loadClass("scala.reflect.internal.Flags$").getField("MODULE$").get(null);
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
        if ("ds".equals(form)) return sourceSymbol(Long.parseLong(d.items.get(1).text()));
        if ("scoped".equals(form)) {
            Object symbol = lazyDecl(owner, d.items.get(2));
            call(symbol, "privateWithin_$eq", 1, sourceSymbol(Long.parseLong(d.items.get(1).text())));
            return symbol;
        }
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
        if ("bounds".equals(kind)) return call(internal, "typeBounds", 2,
            typeFor(s.items.get(1)), typeFor(s.items.get(2)));
        if ("poly".equals(kind)) {
            List<Object> params = new ArrayList<>();
            for (Sexp id : s.items.get(1).items.subList(1, s.items.get(1).items.size())) {
                params.add(sourceSymbol(Long.parseLong(id.text())));
            }
            return call(internal, "polyType", 2, list(params), sourceInfo(owner, s.items.get(2)));
        }
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
            for (Sexp field : s.items.subList(3, s.items.size())) {
                if (field.isList() && !field.items.isEmpty() && "children".equals(field.items.get(0).text())) {
                    for (Sexp child : field.items.subList(1, field.items.size()))
                        call(owner, "addChild", 1, sourceSymbol(Long.parseLong(child.text())));
                }
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
                    Class<?> api = loadClass("scala.reflect.macros.Internals$ContextInternalApi");
                    internalProxy = Proxy.newProxyInstance(macroCl, new Class<?>[]{api}, (p, method, args) -> {
                        if (method.getName().equals("markForAsyncTransform") && method.getParameterCount() == 4) {
                            return markAsync(args);
                        }
                        if (method.getName().equals("enclosingOwner") && method.getParameterCount() == 0) {
                            Sexp answer = query("(q enclosingOwner)");
                            return sourceSymbol(Long.parseLong(answer.items.get(2).text()));
                        }
                        if (method.getName().equals("scala$reflect$macros$Internals$ContextInternalApi$$$outer")) return proxy;
                        String operation = method.getName();
                        if ((operation.equals("attachments") && method.getParameterCount() == 1)
                                || (operation.equals("updateAttachment") && method.getParameterCount() == 3)
                                || (operation.equals("removeAttachment") && method.getParameterCount() == 2)) {
                            // Both runtime trees and symbols own real attachment
                            // stores. Delegate to the object so updates remain
                            // visible through retained contexts and aliases.
                            return call(args[0], operation, args.length - 1,
                                java.util.Arrays.copyOfRange(args, 1, args.length));
                        }
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
            if (n.equals("settings") && arity == 0) {
                List<Object> settings = new ArrayList<>();
                for (String option : compilerSettings) {
                    if (option.startsWith("-Xmacro-settings:")) {
                        for (String setting : option.substring("-Xmacro-settings:".length()).split(",")) {
                            if (!setting.isEmpty()) settings.add(setting);
                        }
                    }
                }
                return list(settings);
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
            if (n.equals("literal") && arity == 1) {
                return literalExpr(a[0]);
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
            if (n.equals("parse") && arity == 1) {
                StringBuilder request = new StringBuilder("(q parse ");
                request.append(Sexp.quote(String.valueOf(a[0])));
                Sexp answer = query(request.append(')').toString());
                if (answer.items.size() != 3) throw gap("malformed c.parse answer");
                if ("fail".equals(answer.items.get(1).text())) {
                    // Context's Java proxy wraps checked ParseException in
                    // UndeclaredThrowableException. Do not let a macro catch
                    // that different exception and silently choose an answer.
                    throw gap("c.parse rejected the source: " + answer.items.get(2).text()
                        + " (ParseException recovery is not implemented)");
                }
                if (!"parsed".equals(answer.items.get(1).text())) throw gap("malformed c.parse answer");
                return buildTree(answer.items.get(2));
            }
            if (n.equals("openMacros") && arity == 0) {
                // nsc prepends this Context to the analyzer's active stack;
                // while its implementation runs, that stack already starts
                // with this same Context. Keep both entries and their identity.
                List<Object> contexts = new ArrayList<>();
                contexts.add(proxy);
                contexts.addAll(openMacroContexts);
                return list(contexts);
            }
            if (n.equals("enclosingMacros") && arity == 0) {
                // The current expansion is the first enclosing macro. Unlike
                // openMacros, this list does not prepend it a second time.
                return list(new ArrayList<Object>(openMacroContexts));
            }
            if (n.equals("openImplicits") && arity == 0) {
                Sexp answer = query("(q openImplicits)");
                if (!"implicits".equals(answer.items.get(1).text()))
                    throw gap("malformed openImplicits answer");
                List<Object> candidates = new ArrayList<>();
                Class<?> candidate = loadClass("scala.reflect.macros.whitebox.Context$ImplicitCandidate");
                Constructor<?> constructor = candidate.getConstructors()[0];
                for (int i = 2; i < answer.items.size(); i++) {
                    Sexp entry = answer.items.get(i);
                    // `(d handle pre sym pt tree)` sends an entry once;
                    // `(h handle)` names one sent before. The candidate is
                    // still built afresh, so a macro never shares its tree.
                    if (entry.items.size() == 2 && "h".equals(entry.items.get(0).atom)) {
                        entry = openImplicitEntries.get(Long.parseLong(entry.items.get(1).text()));
                        if (entry == null) throw gap("openImplicits named an entry it never sent");
                    } else if (entry.items.size() == 6 && "d".equals(entry.items.get(0).atom)) {
                        Sexp body = new Sexp();
                        body.items = new ArrayList<>(entry.items.subList(2, 6));
                        openImplicitEntries.put(Long.parseLong(entry.items.get(1).text()), body);
                        entry = body;
                    }
                    Sexp pre = entry.items.get(0);
                    Object prefixType = "noprefix".equals(pre.items.get(0).text())
                        ? call(universe, "NoPrefix", 0) : typeFor(pre);
                    candidates.add(constructor.newInstance(proxy, prefixType,
                        sourceSymbol(Long.parseLong(entry.items.get(1).text())),
                        typeFor(entry.items.get(2)), buildTree(entry.items.get(3))));
                }
                return list(candidates);
            }
            if (n.equals("inferImplicitValue") && arity == 4) {
                Object enclosing = appTree == null
                    ? call(universe, "NoPosition", 0)
                    : call(appTree, "pos", 0);
                return inferImplicitValue(
                    a[0], (Boolean) a[1], (Boolean) a[2], a[3], enclosing);
            }
            if (n.equals("TypecheckException") && arity == 0) {
                return loadClass("scala.reflect.macros.TypecheckException$")
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
        return loadClass("scala.runtime.BoxedUnit")
            .getField("UNIT").get(null);
    }

    /** `List(xs)` in the immutable Scala list, built from `Nil` and `::`. */
    static Object list(List<Object> xs) throws Exception {
        Object acc = loadClass("scala.collection.immutable.Nil$")
            .getField("MODULE$").get(null);
        Class<?> cons = loadClass("scala.collection.immutable.$colon$colon");
        Constructor<?> c = ctor(cons, 2);
        for (int i = xs.size() - 1; i >= 0; i--) {
            acc = c.newInstance(xs.get(i), acc);
        }
        return acc;
    }

    static boolean isA(Object o, String cls) {
        try {
            return loadClass(cls).isInstance(o);
        } catch (Throwable t) {
            return false;
        }
    }

    /**
     * `Class.forName(name, true, macroCl)`, remembered.
     *
     * The tree serialiser asks `isA(node, "scala.reflect.api.Trees$Select")`
     * and friends for every node of every tree, and each of those went through
     * the class loader's own lookup. Classes do not change identity inside one
     * engine process, so the answer is kept.
     */
    static final java.util.Map<String, Class<?>> classCache = new java.util.HashMap<>();

    static Class<?> loadClass(String name) throws ClassNotFoundException {
        Class<?> known = classCache.get(name);
        if (known == null) {
            known = Class.forName(name, true, macroCl);
            classCache.put(name, known);
        }
        return known;
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
        Method[] candidates = overloads(recv.getClass(), name, arity);
        Method fallback = null;
        for (Method m : candidates) {
            if (fallback == null) {
                fallback = m;
            }
            if (accepts(paramTypes(m), args)) {
                return m.invoke(recv, args);
            }
        }
        if (fallback == null) {
            throw new IllegalStateException(
                "no " + name + "/" + arity + " on " + recv.getClass().getName());
        }
        return fallback.invoke(recv, args);
    }

    /**
     * The methods of `c` named `name` with `arity` parameters, in
     * `getMethods()` order, remembered.
     *
     * `Class.getMethods()` builds and copies a fresh array every call, and a
     * scala-reflect universe class has thousands of public methods; every node
     * of every tree built or serialised here went through that scan. It was
     * 8.2 s of a 27 s gitbucket build -- more than the macro implementations'
     * own run by two orders of magnitude. The cache preserves the order the
     * scan saw, so the overload this picks is the one it picked before.
     */
    /**
     * By class, then arity, then name. The name is almost always a literal,
     * whose hash the string keeps; a composite `class#name/arity` key was
     * built and hashed afresh on every reflective call, which made the lookup
     * cost more than the call.
     */
    static final java.util.IdentityHashMap<Class<?>, java.util.HashMap<String, Method[]>[]> overloadCache =
        new java.util.IdentityHashMap<>();
    /** `Method.getParameterTypes()` copies its array on every call. */
    static final java.util.IdentityHashMap<Method, Class<?>[]> paramTypesCache =
        new java.util.IdentityHashMap<>();

    static Class<?>[] paramTypes(Method m) {
        Class<?>[] known = paramTypesCache.get(m);
        if (known == null) {
            known = m.getParameterTypes();
            paramTypesCache.put(m, known);
        }
        return known;
    }
    static final java.util.Map<Class<?>, Method[]> methodsCache = new java.util.HashMap<>();

    static Method[] methodsOf(Class<?> c) {
        Method[] known = methodsCache.get(c);
        if (known == null) {
            known = c.getMethods();
            methodsCache.put(c, known);
        }
        return known;
    }

    @SuppressWarnings("unchecked")
    static Method[] overloads(Class<?> c, String name, int arity) {
        java.util.HashMap<String, Method[]>[] byArity = overloadCache.get(c);
        if (byArity == null || byArity.length <= arity) {
            java.util.HashMap<String, Method[]>[] grown = new java.util.HashMap[Math.max(arity + 1, 8)];
            if (byArity != null) {
                System.arraycopy(byArity, 0, grown, 0, byArity.length);
            }
            byArity = grown;
            overloadCache.put(c, byArity);
        }
        java.util.HashMap<String, Method[]> byName = byArity[arity];
        if (byName == null) {
            byName = new java.util.HashMap<>();
            byArity[arity] = byName;
        }
        Method[] known = byName.get(name);
        if (known != null) {
            return known;
        }
        List<Method> found = new ArrayList<>();
        for (Method m : methodsOf(c)) {
            if (m.getName().equals(name) && m.getParameterCount() == arity) {
                m.setAccessible(true);
                found.add(m);
            }
        }
        Method[] out = found.toArray(new Method[0]);
        byName.put(name, out);
        return out;
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
        Method[] found = overloads(c, name, arity);
        if (found.length == 0) {
            throw new IllegalStateException("no " + name + "/" + arity + " on " + c.getName());
        }
        return found[0];
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

    /** Read one protocol packet without allowing an unterminated line to grow forever. */
    static String readWireLine(WireReader reader) throws java.io.IOException {
        return readWireLine(reader, MAX_WIRE_CHARS);
    }

    static String readWireLine(WireReader reader, int limit) throws java.io.IOException {
        return reader.readLine(limit);
    }

    /** Only the protocol thread reads this buffer, including nested expansions.
     * Bulk reads avoid BufferedReader's lock/unlock for every wire character. */
    static final class WireReader {
        final java.io.Reader source;
        final char[] chars = new char[8192];
        int at, end;

        WireReader(java.io.Reader source) { this.source = source; }

        String readLine(int limit) throws java.io.IOException {
            StringBuilder sb = null;
            int seen = 0;
            for (;;) {
                if (at == end) {
                    // Never drain an unterminated producer past the first
                    // character that proves this packet is oversized.
                    int size = (int) Math.min(chars.length, (long) limit - seen + 1);
                    end = source.read(chars, 0, size);
                    at = 0;
                    if (end < 0) {
                        end = 0;
                        return seen == 0 ? null : sb.toString();
                    }
                }
                int start = at;
                while (at < end && chars[at] != '\n') {
                    at++;
                    if (++seen > limit) throw new java.io.IOException(
                        "macro protocol line exceeds " + limit + " characters");
                }
                int length = at - start;
                if (at < end) {
                    at++;
                    String line;
                    if (sb == null) line = new String(chars, start, length);
                    else line = sb.append(chars, start, length).toString();
                    // CRLF is transport; all other CR characters are payload.
                    return line.endsWith("\r") ? line.substring(0, line.length() - 1) : line;
                }
                if (sb == null) sb = new StringBuilder();
                sb.append(chars, start, length);
            }
        }
    }

    static void protocolSelfTest() throws Exception {
        for (String bad : new String[] {
                "(ok) trailing", "(ok))", "(ok", "(err \"unterminated)",
                "(err \"bad\\q\")"}) {
            boolean rejected = false;
            try { Sexp.parse(bad); }
            catch (IllegalArgumentException expected) { rejected = true; }
            if (!rejected) throw new AssertionError("accepted malformed packet: " + bad);
        }
        if (!"\r".equals(Sexp.parse("\"\\r\"").text())) {
            throw new AssertionError("CR escape was not decoded");
        }
        boolean rejected = false;
        try { Sexp.parseWithLimits("((a))", 32, 1); }
        catch (IllegalArgumentException expected) { rejected = true; }
        if (!rejected) throw new AssertionError("accepted over-deep packet");

        for (String bad : new String[] {
                "(a)", "(a maybe x)", "(a ok)", "(a ok atom (t))",
                "(a ok (ty \"scala.Int\") atom)", "(a fail)", "(a fail \"x\" extra)"}) {
            rejected = false;
            try { validateTypecheckAnswer(Sexp.parse(bad)); }
            catch (Gap expected) { rejected = true; pendingGap = null; }
            if (!rejected) throw new AssertionError("accepted malformed typecheck answer: " + bad);
        }
        if (!"ok".equals(validateTypecheckAnswer(
                Sexp.parse("(a ok (ty \"scala.Int\") (t \"EmptyTree\" (s0)))")))) {
            throw new AssertionError("rejected valid typecheck ok answer");
        }
        if (!"fail".equals(validateTypecheckAnswer(Sexp.parse("(a fail \"no\")")))) {
            throw new AssertionError("rejected valid typecheck fail answer");
        }

        WireReader bounded = new WireReader(new java.io.StringReader("123456\n(ok)\r\n"));
        rejected = false;
        try { readWireLine(bounded, 5); }
        catch (java.io.IOException expected) { rejected = true; }
        if (!rejected) {
            throw new AssertionError("bounded line reader accepted an oversized packet");
        }

        final int[] reads = new int[] { 0 };
        java.io.Reader producer = new java.io.Reader() {
            public int read(char[] chars, int offset, int length) {
                reads[0]++;
                chars[offset] = 'x';
                return 1;
            }
            public void close() {}
        };
        rejected = false;
        try { readWireLine(new WireReader(producer), 5); }
        catch (java.io.IOException expected) { rejected = true; }
        if (!rejected || reads[0] > 6) {
            throw new AssertionError("bounded line reader drained an unterminated producer");
        }
        final int[] scalarReads = new int[] { 0 };
        BufferedReader counted = new BufferedReader(new java.io.StringReader("(ok)\r\n\n(last)")) {
            public int read() throws java.io.IOException {
                scalarReads[0]++;
                return super.read();
            }
        };
        WireReader bulk = new WireReader(counted);
        if (!"(ok)".equals(readWireLine(bulk, 32))
                || !"".equals(readWireLine(bulk, 0))
                || !"(last)".equals(readWireLine(bulk, 6))
                || readWireLine(bulk, 6) != null) {
            throw new AssertionError("bounded line reader lost a frame or CRLF boundary");
        }
        if (scalarReads[0] != 0) {
            throw new AssertionError("protocol reader still locks once per character");
        }
        char[] longChars = new char[8191];
        java.util.Arrays.fill(longChars, 'x');
        String longLine = new String(longChars);
        WireReader split = new WireReader(new java.io.StringReader(longLine + "\r\nnext\r"));
        if (!longLine.equals(readWireLine(split, 8192))
                || !"next\r".equals(readWireLine(split, 5))
                || readWireLine(split, 5) != null) {
            throw new AssertionError("bounded line reader lost a chunk boundary or EOF payload");
        }
        for (String value : new String[] { "", "plain", "a\nb\tc\rd", "\\\"", "\u65e5\ud83d\ude00" }) {
            if (!value.equals(Sexp.parse(Sexp.quote(value)).text())) {
                throw new AssertionError("protocol string did not round trip: " + value);
            }
        }
        System.out.println("ok");
    }

    // ------------------------------------------------------------------ sexp

    /** The wire format: atoms, quoted strings and lists. */
    static final class Sexp {
        String atom;
        List<Sexp> items;
        /** For a list: the packet it was parsed from, and where it sits in it. */
        String src;
        int start;
        int end;

        /** A list's text as it arrived, which identifies its content. */
        String raw() {
            return src == null ? null : src.substring(start, end);
        }

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
            return parseWithLimits(s, MAX_WIRE_CHARS, MAX_WIRE_DEPTH);
        }

        static Sexp parseWithLimits(String s, int maxChars, int maxDepth) {
            if (s == null || s.length() > maxChars) {
                throw new IllegalArgumentException("macro protocol packet is too large");
            }
            int[] p = {0};
            Sexp v = parse(s, p, 0, maxDepth);
            while (p[0] < s.length() && s.charAt(p[0]) == ' ') p[0]++;
            if (p[0] != s.length()) {
                throw new IllegalArgumentException("trailing input after macro protocol packet");
            }
            return v;
        }

        static Sexp parse(String s, int[] p, int depth, int maxDepth) {
            while (p[0] < s.length() && s.charAt(p[0]) == ' ') {
                p[0]++;
            }
            if (p[0] >= s.length()) {
                throw new IllegalArgumentException("empty macro protocol packet");
            }
            char c = s.charAt(p[0]);
            Sexp v = new Sexp();
            if (c == '(') {
                if (depth >= maxDepth) {
                    throw new IllegalArgumentException("macro protocol packet is nested too deeply");
                }
                v.src = s;
                v.start = p[0];
                p[0]++;
                v.items = new ArrayList<>();
                while (true) {
                    while (p[0] < s.length() && s.charAt(p[0]) == ' ') {
                        p[0]++;
                    }
                    if (p[0] >= s.length()) {
                        throw new IllegalArgumentException("unterminated macro protocol list");
                    }
                    if (s.charAt(p[0]) == ')') {
                        p[0]++;
                        break;
                    }
                    v.items.add(parse(s, p, depth + 1, maxDepth));
                }
                v.end = p[0];
                return v;
            }
            if (c == '"') {
                int start = ++p[0];
                StringBuilder sb = null;
                while (p[0] < s.length() && s.charAt(p[0]) != '"') {
                    char ch = s.charAt(p[0]);
                    if (ch == '\\') {
                        if (sb == null) sb = new StringBuilder();
                        sb.append(s, start, p[0]++);
                        if (p[0] >= s.length()) {
                            throw new IllegalArgumentException("unterminated macro protocol escape");
                        }
                        char e = s.charAt(p[0]++);
                        if (e == 'n') sb.append('\n');
                        else if (e == 't') sb.append('\t');
                        else if (e == 'r') sb.append('\r');
                        else if (e == '"' || e == '\\') sb.append(e);
                        else throw new IllegalArgumentException(
                            "unknown macro protocol escape: \\" + e);
                        start = p[0];
                    } else {
                        p[0]++;
                    }
                }
                if (p[0] >= s.length()) {
                    throw new IllegalArgumentException("unterminated macro protocol string");
                }
                v.atom = sb == null ? s.substring(start, p[0])
                    : sb.append(s, start, p[0]).toString();
                p[0]++;
                return v;
            }
            if (c == ')') {
                throw new IllegalArgumentException("unexpected ) in macro protocol packet");
            }
            int start = p[0];
            while (p[0] < s.length() && " ()".indexOf(s.charAt(p[0])) < 0) {
                p[0]++;
            }
            if (p[0] == start) {
                throw new IllegalArgumentException("empty macro protocol atom");
            }
            v.atom = s.substring(start, p[0]);
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
