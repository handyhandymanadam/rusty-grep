import subprocess,statistics,time,os
D=os.getcwd(); E="ugrep"
T={"fedora":"/usr/bin/rg","ugrep":E,"before":f"{D}/rg-gram","new":f"{D}/rg-dual"}
lists=["w600mix.txt","w900mix.txt","w1000.txt","w1000r.txt"]
print(f"{'list (ms)':<14}"+"".join(f"{t:>9}" for t in T)+"   new vs best other")
for f in lists:
    m={}
    for t,c in T.items():
        r=[]
        for _ in range(5):
            s=time.perf_counter(); subprocess.run(f"{c} -c -F -f {f} big.txt | wc -l",shell=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); r.append((time.perf_counter()-s)*1000)
        m[t]=statistics.median(r)
    other=min(m[t] for t in m if t!="new" and t!="before")
    print(f"{f:<14}"+"".join(f"{m[t]:>9.0f}" for t in T)+f"   {m['new']/other:.2f}x of best other ({'WIN' if m['new']<other else 'LOSS'})",flush=True)
