package com.remoteenv.collector

import android.os.Bundle
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import android.app.ActivityManager
import android.content.Context
import android.content.Intent

class MainActivity : TauriActivity() {
  private var backgroundStartup = false

  override fun onCreate(savedInstanceState: Bundle?) {
    backgroundStartup = intent?.getBooleanExtra(EXTRA_BACKGROUND_BOOT, false) == true
    if (backgroundStartup) {
      setTheme(R.style.Theme_remote_env_desktop_Background)
      window.attributes.windowAnimations = 0
      window.decorView.alpha = 0f
    }
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    runtimeHostAlive = true
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
    if (backgroundStartup) {
      window.decorView.post {
        moveTaskToBack(true)
        window.decorView.alpha = 1f
      }
    }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    if (!intent.getBooleanExtra(EXTRA_BACKGROUND_BOOT, false)) {
      window.decorView.alpha = 1f
    }
  }

  override fun onDestroy() {
    runtimeHostAlive = false
    super.onDestroy()
  }

  fun setHiddenFromRecents(hidden: Boolean) {
    val manager = getSystemService(ACTIVITY_SERVICE) as ActivityManager
    manager.appTasks.firstOrNull()?.setExcludeFromRecents(hidden)
  }

  companion object {
    private const val EXTRA_BACKGROUND_BOOT = "collector_background_boot"
    @Volatile private var runtimeHostAlive = false

    fun ensureBackgroundRuntime(context: Context) {
      if (runtimeHostAlive) return
      context.startActivity(
        Intent(context, MainActivity::class.java)
          .addFlags(
            Intent.FLAG_ACTIVITY_NEW_TASK or
              Intent.FLAG_ACTIVITY_NO_ANIMATION or
              Intent.FLAG_ACTIVITY_EXCLUDE_FROM_RECENTS
          )
          .putExtra(EXTRA_BACKGROUND_BOOT, true)
      )
    }
  }
}
