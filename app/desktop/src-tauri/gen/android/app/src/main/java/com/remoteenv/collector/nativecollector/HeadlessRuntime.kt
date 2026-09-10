package com.remoteenv.collector.nativecollector

import android.content.Context

object HeadlessRuntime {
  init {
    System.loadLibrary("remote_env_desktop_lib")
  }

  @JvmStatic external fun nativeStart(dataDir: String): String
  @JvmStatic external fun nativeStop(): String
  @JvmStatic external fun nativeSubmitEvents(eventsJson: String): String
  @JvmStatic external fun nativeIntervalMillis(): Long
  @JvmStatic external fun nativeStatus(): String

  // Matches Tauri's Android app_data_dir resolver, so UI and service share the same databases.
  fun start(context: Context): String = nativeStart(context.dataDir.absolutePath)
}
