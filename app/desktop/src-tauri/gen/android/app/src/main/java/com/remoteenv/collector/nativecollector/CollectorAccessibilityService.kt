package com.remoteenv.collector.nativecollector

import android.accessibilityservice.AccessibilityService
import android.os.Handler
import android.os.Looper
import android.view.accessibility.AccessibilityEvent

class CollectorAccessibilityService : AccessibilityService() {
  private val handler = Handler(Looper.getMainLooper())
  private val maintenance = object : Runnable {
    override fun run() {
      if (!CollectorForegroundService.isMasterEnabled(applicationContext)) {
        handler.removeCallbacks(this)
        return
      }
      CollectorForegroundService.setEnabled(applicationContext, true)
      RootSupport.startProtection(applicationContext)
      handler.postDelayed(this, 60_000)
    }
  }

  override fun onServiceConnected() {
    super.onServiceConnected()
    active = this
    handler.removeCallbacks(maintenance)
    if (CollectorForegroundService.isMasterEnabled(applicationContext)) handler.post(maintenance)
  }

  // Deliberately does not inspect windows, nodes, text, or user interaction.
  override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit
  override fun onInterrupt() = Unit

  override fun onDestroy() {
    handler.removeCallbacks(maintenance)
    if (active === this) active = null
    super.onDestroy()
  }

  companion object {
    @Volatile private var active: CollectorAccessibilityService? = null

    fun stopMaintenance() {
      val service = active ?: return
      service.handler.removeCallbacks(service.maintenance)
    }
  }
}
