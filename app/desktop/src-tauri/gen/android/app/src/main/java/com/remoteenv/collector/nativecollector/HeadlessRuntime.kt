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

  fun start(context: Context): String = nativeStart(context.filesDir.absolutePath)
}
