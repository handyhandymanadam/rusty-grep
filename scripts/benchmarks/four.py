import subprocess,statistics,time,os,sys
D=os.getcwd(); K=D+"/linux"
T={"grep":"grep","ugrep":"ugrep","rg":"/usr/bin/rg","fork":D+"/rg-pgo"}
# name, cwd, {tool: args}
W=[
 ("tree: plain word",K,{"grep":"-r -F TODO .","ugrep":"-r -F TODO .","rg":"TODO .","fork":"TODO ."}),
 ("tree: regex",K,{"grep":"-rE '\\w+_lock\\(' .","ugrep":"-rE '\\w+_lock\\(' .","rg":"'\\w+_lock\\(' .","fork":"'\\w+_lock\\(' ."}),
 ("tree: ignore case",K,{"grep":"-ri suspend .","ugrep":"-ri suspend .","rg":"-i suspend .","fork":"-i suspend ."}),
 ("tree: count",K,{"grep":"-rc include .","ugrep":"-rc include .","rg":"-c include .","fork":"-c include ."}),
 ("file: plain word",D,{"grep":"-F static big.txt","ugrep":"-F static big.txt","rg":"static big.txt","fork":"static big.txt"}),
 ("file: regex",D,{"grep":"-E '[A-Z]+_[A-Z]+\\(' big.txt","ugrep":"-E '[A-Z]+_[A-Z]+\\(' big.txt","rg":"'[A-Z]+_[A-Z]+\\(' big.txt","fork":"'[A-Z]+_[A-Z]+\\(' big.txt"}),
 ("file: ignore case",D,{"grep":"-i return big.txt","ugrep":"-i return big.txt","rg":"-i return big.txt","fork":"-i return big.txt"}),
 ("file: count",D,{"grep":"-c include big.txt","ugrep":"-c include big.txt","rg":"-c include big.txt","fork":"-c include big.txt"}),
 ("file: inverted",D,{"grep":"-v e big.txt","ugrep":"-v e big.txt","rg":"-v e big.txt","fork":"-v e big.txt"}),
 ("file: every line",D,{"grep":"'' big.txt","ugrep":"'' big.txt","rg":"'' big.txt","fork":"'' big.txt"}),
 ("file: line numbers",D,{"grep":"-n e big.txt","ugrep":"-n e big.txt","rg":"-n e big.txt","fork":"-n e big.txt"}),
 ("file: 400 words",D,{"grep":"-F -f w400r.txt big.txt","ugrep":"-F -f w400r.txt big.txt","rg":"-F -f w400r.txt big.txt","fork":"-F -f w400r.txt big.txt"}),
 ("file: 1000 words",D,{"grep":"-F -f w1000.txt big.txt","ugrep":"-F -f w1000.txt big.txt","rg":"-F -f w1000.txt big.txt","fork":"-F -f w1000.txt big.txt"}),
]
R=int(os.environ.get("ROUNDS","7"))
print(f"{'search (ms, median)':<22}"+"".join(f"{t:>9}" for t in T)+"   fork rank")
for name,cwd,a in W:
    res={t:[] for t in T}
    for t in T: subprocess.run(f"{T[t]} {a[t]} | wc -l",shell=True,cwd=cwd,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    for _ in range(R):
        for t in T:
            s=time.perf_counter(); subprocess.run(f"{T[t]} {a[t]} | wc -l",shell=True,cwd=cwd,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); res[t].append((time.perf_counter()-s)*1000)
    m={t:statistics.median(v) for t,v in res.items()}
    rank=sorted(m,key=m.get).index("fork")+1
    print(f"{name:<22}"+"".join(f"{m[t]:>9.0f}" for t in T)+f"   {rank} of 4",flush=True)
