package tmjava;

/**
 * Modelled on scala/collection/concurrent/INodeBase.java: a *generic* Java
 * superclass for a Scala class, carrying default-access (package-private)
 * static fields that the Scala subclass reads both qualified
 * (`INodeBase.RESTART`) and through a wildcard import of the class's static
 * scope (`import INodeBase._`).
 *
 * Compiled by javac and read back as a class file, which is the only way this
 * compiler sees Java at all -- `tests/scalalib_measure.sh` likewise puts the
 * library's 33 javac-produced classfiles on the classpath rather than
 * compiling the .java sources.
 */
public abstract class JBase<K, V> {

    static final String SENTINEL = "<none>";

    static final String RESTART = "<retry>";

    public static final String PUBLIC_TAG = "pub";

    /**
     * A field whose type is generic, so the class file carries a `Signature`
     * attribute the erased descriptor cannot express. Reading only the
     * descriptor gives a *raw* `JBase`, which conforms to every instantiation
     * -- a soundness hole, not merely a missing type.
     */
    public static final JBase<String, Integer> PROTOTYPE = null;

    public final K key;

    public final V value;

    protected JBase(K k, V v) {
        key = k;
        value = v;
    }

    public abstract String describe();
}
