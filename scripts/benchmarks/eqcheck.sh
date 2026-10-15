#!/bin/bash
N=${N:-~/work/ripgrep-fork/target/release/rg}; S=~/work/ripgrep-fork-data/rg-stock; f=$1; bad=0; n=0
while IFS= read -r args; do
  a=$($N $args big.txt 2>&1 | grep -v 'elapsed\|seconds' | md5sum | cut -c1-8); b=$($S $args big.txt 2>&1 | grep -v 'elapsed\|seconds' | md5sum | cut -c1-8)
  n=$((n+1)); [ "$a" != "$b" ] && { echo "DIFF: $f: $args"; bad=1; }
done <<ARGS
-F -f $f
-f $f
-n -F -f $f
-c -F -f $f
-l -F -f $f
-v -c -F -f $f
--color=always -m200 -F -f $f
-C1 -m50 -F -f $f
--json -m20 -F -f $f
-c -F -f $f -e zzqq_extra
-o -m300 -F -f $f
--column -m300 -F -f $f
ARGS
echo "$f: $n combos, $([ $bad = 0 ] && echo all-identical || echo DIFFERENCES)"
