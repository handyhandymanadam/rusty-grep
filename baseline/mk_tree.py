# Deterministic 40k-line test tree for rusty-grep timing: python3 -I mk_tree.py DIR
import os, random, sys
root = sys.argv[1]
random.seed(7)
words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon".split()
os.makedirs(root, exist_ok=True)
os.makedirs(os.path.join(root, ".git"), exist_ok=True)
open(os.path.join(root, ".gitignore"), "w").write("build/\n*.log\n")
def wr(path, lines):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        for _ in range(lines):
            f.write(" ".join(random.choice(words) for _ in range(random.randint(3, 12))) + "\n")
for d in range(20):
    for i in range(10):
        wr(f"{root}/src/d{d:02d}/f{i}.txt", 200)
wr(f"{root}/.hidden/h.txt", 200)
wr(f"{root}/build/out.txt", 200)
wr(f"{root}/src/d00/x.log", 100)
with open(f"{root}/src/blob.bin", "wb") as f:
    f.write(b"alpha\x00beta\n" * 10)
