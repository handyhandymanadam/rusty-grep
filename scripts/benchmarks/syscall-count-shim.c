#define _GNU_SOURCE
#include <dlfcn.h>
#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/stat.h>
#include <stdarg.h>
#include <fcntl.h>
static unsigned long c_read, c_open, c_close, c_stat, c_other, c_lseek, c_mmap, c_munmap, c_ioctl, c_getd, c_fstatat, c_madv;
#define WRAP(name, ctr, ret, proto, args) ret name proto { static ret (*real) proto; if(!real) real=dlsym(RTLD_NEXT,#name); __atomic_fetch_add(&ctr,1,__ATOMIC_RELAXED); return real args; }
WRAP(read, c_read, ssize_t, (int fd, void*b, size_t n), (fd,b,n))
WRAP(close, c_close, int, (int fd), (fd))
WRAP(lseek, c_lseek, off_t, (int fd, off_t o, int w), (fd,o,w))
WRAP(munmap, c_munmap, int, (void*a, size_t l), (a,l))
WRAP(madvise, c_madv, int, (void*a, size_t l, int x), (a,l,x))
WRAP(readdir, c_getd, struct dirent*, (DIR*d), (d))
int openat(int dfd, const char*p, int fl, ...) { static int(*real)(int,const char*,int,...); if(!real) real=dlsym(RTLD_NEXT,"openat"); __atomic_fetch_add(&c_open,1,__ATOMIC_RELAXED); va_list ap; va_start(ap,fl); mode_t m=va_arg(ap,mode_t); va_end(ap); return real(dfd,p,fl,m); }
int fstat(int fd, struct stat*s){ static int(*real)(int,struct stat*); if(!real) real=dlsym(RTLD_NEXT,"fstat"); __atomic_fetch_add(&c_stat,1,__ATOMIC_RELAXED); return real(fd,s);}
int statx(int d,const char*p,int f,unsigned m,struct statx*b){ static int(*real)(int,const char*,int,unsigned,struct statx*); if(!real) real=dlsym(RTLD_NEXT,"statx"); __atomic_fetch_add(&c_fstatat,1,__ATOMIC_RELAXED); return real(d,p,f,m,b);}
__attribute__((destructor)) static void fin(void){ fprintf(stderr,"read=%lu openat=%lu close=%lu fstat=%lu statx=%lu lseek=%lu munmap=%lu madvise=%lu readdir=%lu\n",c_read,c_open,c_close,c_stat,c_fstatat,c_lseek,c_munmap,c_madv,c_getd); }
