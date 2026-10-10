class Switches {
    int f(int x) {
        switch (x) {
            case 0:
                x++;
            case 1:
                x++;
                // fall through
            case 2:
                x++;
                /* falls through */
            case 3:
                return 3;
            case 4: {
                x++;
                break;
            }
            case 5:
                if (x > 1) {
                    return 1;
                } else {
                    throw new IllegalStateException();
                }
            case 6:
                try {
                    return 2;
                } finally {
                    x--;
                }
            case 7:
                synchronized (this) {
                    break;
                }
            case 8:
                while (true) {
                    break;
                }
            case 9:
                for (;;) {
                    x++;
                }
            default:
                x--;
            case 10:
                do { x++; } while (x < 3);
            case 11: /* fallthru */ case 12:
                x++;
        }
        switch (x) {
            default:
            case 1:
                return 0;
            case 2:
                x++;
        }
        int y = switch (x) {
            case 1 -> 10;
            case 2, 3 -> {
                yield 20;
            }
            default -> throw new IllegalArgumentException();
        };
        int z = switch (x) {
            case 1:
                yield 1;
            default:
                yield 2;
        };
        return y + z;
    }
}
