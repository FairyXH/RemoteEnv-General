#!/system/bin/sh

MODDIR=${0%/*}
PACKAGE=com.remoteenv.collector
SERVICE=com.remoteenv.collector/.nativecollector.CollectorForegroundService
ACCESSIBILITY_SERVICE=$PACKAGE/$PACKAGE.nativecollector.CollectorAccessibilityService
ACCESSIBILITY_SERVICE_SHORT=$PACKAGE/.nativecollector.CollectorAccessibilityService
STATE_DIR=/data/adb/remote_env_collector
PID_FILE=$STATE_DIR/guard.pid

mkdir -p "$STATE_DIR"
echo $$ > "$PID_FILE"

protect() {
  dumpsys deviceidle whitelist +$PACKAGE >/dev/null 2>&1
  cmd appops set $PACKAGE RUN_IN_BACKGROUND allow >/dev/null 2>&1
  cmd appops set $PACKAGE RUN_ANY_IN_BACKGROUND allow >/dev/null 2>&1
  for permission in \
    android.permission.ACCESS_FINE_LOCATION \
    android.permission.ACCESS_COARSE_LOCATION \
    android.permission.ACCESS_BACKGROUND_LOCATION \
    android.permission.READ_PHONE_STATE \
    android.permission.BLUETOOTH_SCAN \
    android.permission.BLUETOOTH_CONNECT \
    android.permission.NEARBY_WIFI_DEVICES \
    android.permission.POST_NOTIFICATIONS
  do
    pm grant $PACKAGE "$permission" >/dev/null 2>&1
  done
  for app_pid in $(pidof $PACKAGE 2>/dev/null); do
    echo -1000 > "/proc/$app_pid/oom_score_adj" 2>/dev/null
  done

  enabled_services=$(settings get secure enabled_accessibility_services 2>/dev/null)
  [ "$enabled_services" = "null" ] && enabled_services=
  case ":$enabled_services:" in
    *":$ACCESSIBILITY_SERVICE:"*|*":$ACCESSIBILITY_SERVICE_SHORT:"*) ;;
    *)
      if [ -n "$enabled_services" ]; then
        enabled_services="$enabled_services:$ACCESSIBILITY_SERVICE"
      else
        enabled_services="$ACCESSIBILITY_SERVICE"
      fi
      settings put secure enabled_accessibility_services "$enabled_services" >/dev/null 2>&1
      ;;
  esac
  settings put secure accessibility_enabled 1 >/dev/null 2>&1
}

until [ "$(getprop sys.boot_completed)" = "1" ]; do sleep 5; done
sleep 10

last_launch=0
while [ ! -f "$MODDIR/disable" ]; do
  if pm path $PACKAGE >/dev/null 2>&1; then
    pm enable $PACKAGE >/dev/null 2>&1
    protect
    if ! pidof $PACKAGE >/dev/null 2>&1; then
      now=$(date +%s)
      if [ $((now - last_launch)) -ge 60 ]; then
        am start-foreground-service -n "$SERVICE" >/dev/null 2>&1
        last_launch=$now
      fi
    else
      am start-foreground-service -n "$SERVICE" >/dev/null 2>&1
    fi
  fi
  sleep 30
done
