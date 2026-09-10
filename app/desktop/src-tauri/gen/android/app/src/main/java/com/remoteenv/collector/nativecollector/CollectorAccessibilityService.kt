package com.remoteenv.collector.nativecollector

import android.accessibilityservice.AccessibilityService
import android.os.Handler
import android.os.Looper
import android.view.accessibility.AccessibilityEvent

class CollectorAccessibilityService : AccessibilityService() {
  private val handler = Handler(Looper.getMainLooper())
  private val maintenance = object : Runnable {
    override fun run() {
      CollectorForegroundService.setEnabled(applicationContext, true)
      RootSupport.startProtection(applicationContext)
      handler.postDelayed(this, 60_000)
    }
  }

  override fun onServiceConnected() {
    super.onServiceConnected()
    handler.removeCallbacks(maintenance)
    handler.post(maintenance)
  }

  // Deliberately does not inspect windows, nodes, text, or user interaction.
  override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit
  override fun onInterrupt() = Unit

  override fun onDestroy() {
    handler.removeCallbacks(maintenance)
    super.onDestroy()
  }
}
