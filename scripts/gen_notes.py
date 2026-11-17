#!/usr/bin/env python3
import os, random, sys

def main(out_dir, n=10000):
    os.makedirs(out_dir, exist_ok=True)
    words = ["rust", "gtk", "markdown", "nota", "texto", "memoria", "velocidad",
             "frecuencia", "palabra", "sistema", "archivo", "indexador", "cache",
             "interfaz", "render", "pango", "color", "tamaño", "ventana", "pantalla"]
    for i in range(n):
        d = os.path.join(out_dir, f"{i//1000:02d}")
        os.makedirs(d, exist_ok=True)
        lines = []
        for _ in range(random.randint(3, 8)):
            wl = [random.choice(words) for _ in range(random.randint(5, 15))]
            lines.append(" ".join(wl))
        with open(os.path.join(d, f"nota_{i:05d}.md"), "w") as f:
            f.write("# Nota {}\n\n".format(i))
            f.write("\n".join(lines))
            f.write("\n")
    print(f"Generados {n} archivos en {out_dir}")

if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/notes10k"
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 10000
    main(out, n)
