#!/bin/bash
# Samples resources every 10 s while audit.pid is alive. Stops the audit if system memory gets tight.
cd ~/work/ripgrep-fork-data/audit; : > resources.log
PID=$(cat audit.pid); CG=/sys/fs/cgroup$(cut -d: -f3 /proc/$PID/cgroup)
while kill -0 "$PID" 2>/dev/null; do
  avail=$(free -m | awk '/Mem:/{print $7}'); swap=$(free -m | awk '/Swap:/{print $3}'); load=$(cut -d' ' -f1 /proc/loadavg)
  scope=$(( $(cat $CG/memory.current 2>/dev/null || echo 0) / 1048576 )); hi=$(awk '/^high /{print $2}' $CG/memory.events 2>/dev/null)
  echo "$(date +%T) avail=${avail}MB swapused=${swap}MB load=${load} audit-scope=${scope}MB high-events=${hi}" >> resources.log
  if [ "$avail" -lt 3000 ] || [ "$swap" -gt 7600 ]; then echo "$(date +%T) LOW MEMORY - stopping audit" >> resources.log; kill "$PID"; break; fi
  sleep 10
done
