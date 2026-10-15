# Benchmark and audit scripts (kept from the 2026-10 speed review)

They need data that is NOT in git (a shallow Linux kernel clone, a 239 MB text file built from it, word lists) and,
for rawgrep, an ext4 loop image plus a rawgrep build. Paths inside the scripts point at ~/work/ripgrep-fork-data.
- `five.py`, `cold5.py`: 5-way timing (grep, ugrep, ripgrep, this fork, rawgrep), warm and cold-cache.
- `audit.py`, `genpatterns.py`, `patterns.json`, `stale.py`, `speed_eq.py`, `watchdog.sh`: accuracy audit of rawgrep
  against ripgrep (262 patterns), staleness scenarios, equal-settings speed. audit.py compares through disk files; run it
  in a capped scope (systemd-run --user --scope -p MemoryHigh=2G -p MemoryMax=3G) so it cannot starve the rest of the system.
- `eqcheck.sh`: output-equality check of this fork against stock ripgrep over many option combinations.


Added 2026-10-04 (second worker session):
- `evict.py DIR`: drops one folder's file data from the page cache without root (posix_fadvise DONTNEED), for cold-cache
  timing of a tree. Metadata (names, inodes) stays cached, so a real cold disk may differ.
- `four.py`: warm 4-way table (GNU grep, ugrep, Fedora rg, fork) on tree and single-file searches. ugrep is a separate install
  (`ugrep ...`); edit the T table if it is installed separately. Output must go to a pipe
  (GNU grep and ugrep stop at the first match when stdout is /dev/null).
- `lists.py`: word-list searches (`-c -F -f LIST big.txt`) across several lists and tools. Lists live in the data folder:
  w100r/w200r/w400r/w1000r = random words that occur on <= 2 lines of big.txt; w400short = rare words <= 5 letters;
  w400s = 400 words from the kernel tree that appear on <= 3 lines of drivers/gpu; w400/w1000 = random common kernel words
  (dense); w*mix = 4-letter words (3-60 occurrences) mixed with rare words of 7+ letters.
- `syscall-count-shim.c`: LD_PRELOAD shim counting read/close/statx/... (perf trace and strace were not allowed here).
  Build: gcc -shared -fPIC -O1 -o cnt.so syscall-count-shim.c -ldl; run: LD_PRELOAD=./cnt.so rg ...
