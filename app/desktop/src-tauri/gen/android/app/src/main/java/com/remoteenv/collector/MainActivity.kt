package com.remoteenv.collector

import android.os.Bundle
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import android.app.ActivityManager

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    val permissions = mutableListOf(
      Manifest.permission.ACCESS_FINE_LOCATION,
      Manifest.permission.ACCESS_COARSE_LOCATION,
      Manifest.permission.READ_PHONE_STATE,
    )
    if (Build.VERSION.SDK_INT >= 31) {
      permissions += Manifest.permission.BLUETOOTH_SCAN
      permissions += Manifest.permission.BLUETOOTH_CONNECT
    }
    if (Build.VERSION.SDK_INT >= 33) permissions += Manifest.permission.NEARBY_WIFI_DEVICES
    if (Build.VERSION.SDK_INT >= 33) permissions += Manifest.permission.POST_NOTIFICATIONS
    val missing = permissions.filter { ActivityCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED }
    if (missing.isNotEmpty()) ActivityCompat.requestPermissions(this, missing.toTypedArray(), 1001)
    getSharedPreferences("collector_persistence", MODE_PRIVATE).let { preferences ->
      setHiddenFromRecents(preferences.getBoolean("hide_from_recents", false))
      if (preferences.getBoolean("foreground_enabled", false)) {
        com.remoteenv.collector.nativecollector.CollectorForegroundService.setEnabled(this, true)
      }
      if (preferences.getBoolean("root_enabled", false)) {
        Thread { com.remoteenv.collector.nativecollector.RootSupport.apply(this, true) }.start()
      }
    }
  }

  fun setHiddenFromRecents(hidden: Boolean) {
    val manager = getSystemService(ACTIVITY_SERVICE) as ActivityManager
    manager.appTasks.firstOrNull()?.setExcludeFromRecents(hidden)
  }

}
