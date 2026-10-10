import java.util.List;

class Loops {
    void f(List<String> items, int[] values) {
        for (int i = 0; i < 10; i++) {
            i += 2;
            int j = i; j++;
        }
        for (int i = 0, k = 5; i < k; i++, k--) {
            k = 3;
            i--;
        }
        for (String s : items) {
            s = s.trim();
        }
        for (int v : values) { v++; }
        Runnable r = () -> { int a = 1; int b = 2; };
        Runnable q = () -> {
            int c = 3; int d = 4;
        };
        int m = 0; int n = 1;
        for (int i = 0; i < 2; i++) { m++; n++; }
        try (java.io.Reader a = null; java.io.Reader b = null) {
            m++;
        } catch (Exception e) {
            n++;
        }
        new Object() {
            void g() {
                for (int t = 0; t < 1; t++) {
                    t = 2;
                }
            }
        };
        ;
        if (m == 1) ;
    }
}
