#!/system/bin/sh

PACKAGE=com.remoteenv.collector
PID_FILE=/data/adb/remote_env_collector/guard.pid
if [ -f "$PID_FILE" ]; then
  kill "$(cat "$PID_FILE")" >/dev/null 2>&1
fi
rm -rf /data/adb/remote_env_collector
dumpsys deviceidle whitelist -$PACKAGE >/dev/null 2>&1
