import java.nio.charset.StandardCharsets;
public final class NameTransformerOracle {
    private static String hex(String text) {
        StringBuilder out = new StringBuilder();
        for (byte b : text.getBytes(StandardCharsets.UTF_8)) {
            out.append(Character.forDigit((b & 255) >>> 4, 16));
            out.append(Character.forDigit(b & 15, 16));
        }
        return out.toString();
    }
    private static void print(String name) {
        System.out.println(hex(scala.reflect.NameTransformer.encode(name)) + ":" +
            hex(scala.reflect.NameTransformer.decode(name)));
    }
    public static void main(String[] args) {
        for (int i = 0; i <= 0xFFFF; i++) {
            if (!Character.isSurrogate((char)i)) print(String.valueOf((char)i));
        }
        for (String arg : args) print(arg);
    }
}
