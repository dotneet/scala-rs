import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.net.URL;
import java.net.URLClassLoader;
import java.io.File;

/**
 * Feasibility probe: implement scala.reflect.macros.blackbox.Context with a
 * java.lang.reflect.Proxy whose `universe` is scala.reflect.runtime.universe,
 * then reflectively invoke a scalac-compiled macro implementation and print the
 * tree it returns.
 *
 * args[0] = directory holding the compiled macro impl
 * args[1] = impl module class name (e.g. "M$")
 * args[2] = impl method name        (e.g. "impl")
 */
public class Proto {
    static Object universe;
    static Object mirror;
    static ClassLoader macroCl;

    public static void main(String[] args) throws Exception {
        File dir = new File(args[0]);
        macroCl = new URLClassLoader(new URL[]{dir.toURI().toURL()}, Proto.class.getClassLoader());

        Class<?> runtimePkg = Class.forName("scala.reflect.runtime.package$");
        Object pkg = runtimePkg.getField("MODULE$").get(null);
        universe = runtimePkg.getMethod("universe").invoke(pkg);
        System.out.println("universe = " + universe.getClass().getName());

        Class<?> ctxIface = Class.forName("scala.reflect.macros.blackbox.Context");
        System.out.println("universe is a macros.Universe: "
            + Class.forName("scala.reflect.macros.Universe").isInstance(universe));

        // runtimeMirror(classLoader)
        Method rm = universe.getClass().getMethod("runtimeMirror", ClassLoader.class);
        mirror = rm.invoke(universe, macroCl);

        Object ctx = Proxy.newProxyInstance(
            Proto.class.getClassLoader(), new Class<?>[]{ctxIface}, new Handler());

        Class<?> implCls = Class.forName(args[1], true, macroCl);
        Object implMod = implCls.getField("MODULE$").get(null);

        Method impl = null;
        for (Method m : implCls.getMethods()) {
            if (m.getName().equals(args[2])) { impl = m; break; }
        }
        if (impl == null) throw new IllegalStateException("no method " + args[2]);
        System.out.println("impl = " + impl);

        // Build the remaining arguments: Expr params get Literal(Constant(41)),
        // WeakTypeTag params get the tag for String.
        Class<?>[] pts = impl.getParameterTypes();
        Object[] argv = new Object[pts.length];
        argv[0] = ctx;
        for (int i = 1; i < pts.length; i++) {
            String pn = pts[i].getName();
            if (pn.equals("scala.reflect.api.Exprs$Expr")) {
                argv[i] = Handler.mkExpr(litInt(41));
            } else if (pn.equals("scala.reflect.api.TypeTags$WeakTypeTag")
                    || pn.equals("scala.reflect.api.TypeTags$TypeTag")) {
                argv[i] = stringTag();
            } else {
                throw new IllegalStateException("unsupported macro impl param: " + pn);
            }
        }
        Object result = impl.invoke(implMod, argv);
        System.out.println("result class = " + result.getClass().getName());

        // Expr.tree, then universe.showRaw(tree)
        Object tree = result;
        try {
            Method treeM = result.getClass().getMethod("tree");
            tree = treeM.invoke(result);
        } catch (NoSuchMethodException e) { /* already a Tree */ }

        System.out.println("CODE: " + tree.toString());
        Method showRaw = null;
        for (Method m : universe.getClass().getMethods()) {
            if (m.getName().equals("showRaw") && m.getParameterCount() == 7) { showRaw = m; break; }
        }
        Object[] flags = new Object[7];
        flags[0] = tree;
        for (int i = 2; i <= 7; i++) {
            Method d = universe.getClass().getMethod("showRaw$default$" + i);
            flags[i - 1] = d.invoke(universe);
        }
        System.out.println("EXPANSION: " + showRaw.invoke(universe, flags));
    }

    /** universe.Literal(universe.Constant(i)) */
    static Object litInt(int i) throws Exception {
        Object constantCompanion = Handler.call(universe, "Constant");
        Object constant = null;
        for (Method m : constantCompanion.getClass().getMethods())
            if (m.getName().equals("apply") && m.getParameterCount() == 1)
                constant = m.invoke(constantCompanion, Integer.valueOf(i));
        Object literalCompanion = Handler.call(universe, "Literal");
        for (Method m : literalCompanion.getClass().getMethods())
            if (m.getName().equals("apply") && m.getParameterCount() == 1)
                return m.invoke(literalCompanion, constant);
        throw new IllegalStateException("no Literal.apply");
    }

    /** A WeakTypeTag for java.lang.String, built from the mirror. */
    static Object stringTag() throws Exception {
        Method sc = mirror.getClass().getMethod("staticClass", String.class);
        Object cls = sc.invoke(mirror, "java.lang.String");
        Method toType = null;
        for (Method m : cls.getClass().getMethods())
            if (m.getName().equals("toType") && m.getParameterCount() == 0) toType = m;
        Object tpe = toType.invoke(cls);
        Object tagCompanion = Handler.call(universe, "WeakTypeTag");
        Class<?> creatorCls = Class.forName("scala.reflect.internal.StdCreators$FixedMirrorTypeCreator");
        Object creator = null;
        for (var ctor : creatorCls.getConstructors())
            if (ctor.getParameterCount() == 3) creator = ctor.newInstance(universe, mirror, tpe);
        for (Method m : tagCompanion.getClass().getMethods())
            if (m.getName().equals("apply") && m.getParameterCount() == 2)
                return m.invoke(tagCompanion, mirror, creator);
        throw new IllegalStateException("no WeakTypeTag.apply");
    }

    static class Handler implements InvocationHandler {
        /** universe.Expr(mirror, FixedMirrorTreeCreator(mirror, tree))(AnyTag) */
        static Object mkExpr(Object tree) throws Exception {
            Object exprCompanion = call(universe, "Expr");
            Object creator = newFixedMirrorTreeCreator(tree);
            for (Method em : exprCompanion.getClass().getMethods())
                if (em.getName().equals("apply") && em.getParameterCount() == 3)
                    return em.invoke(exprCompanion, mirror, creator, stringTag());
            throw new IllegalStateException("no Expr.apply");
        }

        public Object invoke(Object proxy, Method m, Object[] a) throws Throwable {
            String n = m.getName();
            if (n.equals("universe")) return universe;
            if (n.equals("mirror")) return mirror;
            if (n.equals("toString")) return "scala-rs macro Context";
            if (n.equals("hashCode")) return System.identityHashCode(proxy);
            if (n.equals("equals")) return proxy == a[0];

            // vals of the Aliases trait: forward the companions from the universe
            if (n.equals("Expr") && m.getParameterCount() == 0) return call(universe, "Expr");
            if (n.equals("WeakTypeTag") && m.getParameterCount() == 0) return call(universe, "WeakTypeTag");
            if (n.equals("TypeTag") && m.getParameterCount() == 0) return call(universe, "TypeTag");
            if (n.startsWith("scala$reflect$macros$") && n.contains("_setter_")) return null;

            // c.Expr[T](tree)(tag) -> universe.Expr(mirror, FixedMirrorTreeCreator(mirror, tree))(tag)
            if (n.equals("Expr") && m.getParameterCount() == 2) {
                Object exprCompanion = call(universe, "Expr");
                Object creator = newFixedMirrorTreeCreator(a[0]);
                for (Method em : exprCompanion.getClass().getMethods()) {
                    if (em.getName().equals("apply") && em.getParameterCount() == 3) {
                        return em.invoke(exprCompanion, mirror, creator, a[1]);
                    }
                }
                throw new IllegalStateException("no Expr.apply(mirror, creator, tag)");
            }

            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, a);
            throw new UnsupportedOperationException(
                "scala-rs macro engine: Context." + n + " is not implemented");
        }

        static Object call(Object recv, String name) throws Exception {
            for (Method m : recv.getClass().getMethods()) {
                if (m.getName().equals(name) && m.getParameterCount() == 0) return m.invoke(recv);
            }
            throw new IllegalStateException("no " + name + " on " + recv.getClass());
        }

        static Object newFixedMirrorTreeCreator(Object tree) throws Exception {
            // universe.FixedMirrorTreeCreator is an inner case class of StdCreators.
            Class<?> c = Class.forName("scala.reflect.internal.StdCreators$FixedMirrorTreeCreator");
            for (var ctor : c.getConstructors()) {
                if (ctor.getParameterCount() == 3) return ctor.newInstance(universe, mirror, tree);
            }
            throw new IllegalStateException("no FixedMirrorTreeCreator ctor");
        }
    }
}
