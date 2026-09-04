#!/system/bin/sh

ui_print "- Installing RemoteEnvCollector Root Guard"
ui_print "- Target package: com.remoteenv.collector"
set_perm_recursive "$MODPATH" 0 0 0755 0644
set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/uninstall.sh" 0 0 0755
