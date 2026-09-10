public class ctxinfer_JavaParent {
    private final String prefix;
    private final int[] values;
    public ctxinfer_JavaParent(String prefix, int... values) {
        this.prefix = prefix;
        this.values = values;
    }
    public String answer() {
        int sum = 0;
        for (int value : values) sum += value;
        return prefix + sum;
    }
}
