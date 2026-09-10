package com.remoteenv.collector.nativecollector

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class CollectorBootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent?) {
    val action = intent?.action ?: return
    if (action != Intent.ACTION_BOOT_COMPLETED && action != Intent.ACTION_MY_PACKAGE_REPLACED) return
    val preferences = context.getSharedPreferences("collector_persistence", Context.MODE_PRIVATE)
    if (!CollectorForegroundService.isMasterEnabled(context)) return
    if (!preferences.getBoolean("auto_start_enabled", false)) return
    CollectorForegroundService.setEnabled(context, true)
  }
}
