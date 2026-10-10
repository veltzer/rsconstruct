package p;

import java.util.*;
import java.util.function.*;

@SuppressWarnings({"unchecked", "rawtypes"})
public class Syntax<T extends Comparable<? super T> & java.io.Serializable> {
    private static final String TEXT = """
        Hello, "world"
        \t tab \"\"" end
        """;
    Map<String, List<Map<Integer, Set<T>>>> nested = new HashMap<>();
    int hex = 0xCAFE_BABE, oct = 0_17, bin = 0b1010_1010;
    long big = 9_223_372_036_854_775_807L;
    float f = 1.5f, g = .5F, h = 1e10f, i = 0x1.8p1f;
    double d = 1.5, e = 1d, k = 1.e2, m = 0x1p-3d, n = 1_0.0_1e-1_0D;
    char c = 'A', q = '\'', r = '\\', s = '\077';
    String u = "é\r\n\b\f\0";

    record Point(int x, @Deprecated int y, String... rest) implements Comparable<Point> {
        Point {
            if (x < 0) throw new IllegalArgumentException();
        }
        public int compareTo(Point o) { return Integer.compare(x, o.x); }
    }

    interface I<A, B> { B apply(A a); default void m() {} }

    @interface Note { String value() default "x"; int[] counts() default {1, 2}; }

    enum Color implements Runnable {
        RED { public void run() {} }, GREEN(1) { public void run() {} }, BLUE;
        Color() {}
        Color(int v) {}
        public void run() {}
    }

    <R> R generic(Function<? super T, ? extends R> fn, T t) {
        Supplier<List<String>> sup = ArrayList::new;
        BiFunction<Integer, Integer, Integer> add = (a, b) -> a + b;
        Function<String, Integer> len = String::length;
        Runnable run = () -> System.out.println("x");
        Object o = (Runnable & java.io.Serializable) () -> {};
        int[][] grid = new int[3][];
        var list = List.<String>of("a");
        if (o instanceof String str && !str.isEmpty()) {
            return fn.apply(t);
        }
        label:
        for (int i = 0; i < 3; i++) {
            if (i == 1) continue label;
            if (i == 2) break label;
        }
        assert t != null : "t";
        int x = ~1 >>> 2 >> 1 << 3;
        x >>>= 1; x >>= 1; x <<= 1; x ^= 1; x |= 1; x &= 1; x %= 3;
        boolean b = x > 1 ? x < 2 : x >= 3;
        return null;
    }

    Syntax() { this(1); }
    Syntax(int a) { super(); }

    static { int s = 0; }
    { int i = 0; }
}
