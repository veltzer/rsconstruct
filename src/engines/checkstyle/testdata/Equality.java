class Equality {
    boolean a(String s, boolean b) {
        if (s == "x" || "y" != s) {
            return true;
        } else {
            return false;
        }
    }

    boolean b(boolean b) {
        if (b == true) return b != false;
        if (!false && b || true) { return true; } else return false;
        return b;
    }

    long c() {
        long x = 10l + 20L + 0x1FL + 0b101l;
        return x;
    }

    public boolean equals(Equality other) {
        return other == this;
    }

    @Override
    public int hashCode() {
        return 1;
    }

    static class Both {
        public boolean equals(Object o) { return false; }
        public int hashCode() { return 0; }
    }

    static class OnlyEquals {
        public boolean equals(java.lang.Object o) { return false; }
    }

    static class Covariant {
        public boolean equals(Covariant c) { return true; }
        public boolean equals(String c) { return true; }
    }

    enum E {
        A;
        public boolean equals(E e) { return true; }
    }

    Object anon = new Object() {
        public boolean equals(Object o) { return true; }
    };
}
