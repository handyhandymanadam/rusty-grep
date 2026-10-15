#!/bin/bash
# usage: eqout.sh BASE_RG NEW_RG  -- compares the output of many option combinations on big.txt (run from the data folder)
B=$1; N=$2; bad=0; n=0
while IFS= read -r args; do
  a=$(eval $B $args 2>&1 | grep -v 'elapsed\|seconds' | md5sum | cut -c1-8); b=$(eval $N $args 2>&1 | grep -v 'elapsed\|seconds' | md5sum | cut -c1-8)
  n=$((n+1)); [ "$a" != "$b" ] && { echo "DIFFERENT: $args"; bad=1; }
done <<'ARGS'
-n e big.txt
-n -v e big.txt
-n -C2 -m1000 static big.txt
-n -B3 -A3 -m500 mutex_lock big.txt
-nU 'static.*\n.*inline' big.txt
--passthru -n -m50 static big.txt
-n -o '[A-Z]+_[A-Z]+' big.txt
-nb -m300 return big.txt
--json -m100 -n static big.txt
-c e big.txt
-n '' big.txt
-n -w size_t big.txt
-n --column static big.txt
--color=always -n -m300 static big.txt
-n --crlf e big.txt
-n -a e big.txt
-nF -f w400.txt big.txt
--all-of -n -e static -e inline big.txt
-n -i copyright big.txt
-l static big.txt
-n --max-columns 80 static big.txt
-nr X static big.txt
--vimgrep -m200 return big.txt
ARGS
echo "$n option combinations compared: $([ $bad = 0 ] && echo all identical || echo DIFFERENCES)"
