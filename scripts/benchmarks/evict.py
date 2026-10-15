import os, sys
n=0
for root, dirs, files in os.walk(sys.argv[1]):
    for f in files:
        p=os.path.join(root,f)
        try:
            fd=os.open(p, os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd); n+=1
        except OSError: pass
print("evicted",n,"files")
